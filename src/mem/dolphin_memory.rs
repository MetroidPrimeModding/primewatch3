use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Copy cap: the amount of emulated RAM (MEM1) the game snapshot mirrors.
/// C++ `MemoryAccess.hpp:7` — `constexpr int DOLPHIN_MEMORY_SIZE = 0x1800000`.
pub const DOLPHIN_MEMORY_SIZE: usize = 0x1800000;

/// The span of Dolphin's shared-memory mapping (`mmap`/`munmap` length).
/// C++ `MemoryAccess.cpp:74` / `:103` — `constexpr size_t size = 0x2040000`.
/// Larger than `DOLPHIN_MEMORY_SIZE` because it also covers the L1 cache / MEM2 region.
const DOLPHIN_SHM_SIZE: usize = 0x2040000;

pub struct DolphinMemoryAccess {
  system: System,
  attached_pid: i32,

  // POSIX (Linux + macOS share the shm_open/mmap path — C++ __APPLE__ branch is
  // byte-identical to the __linux__ branch).
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  emu_ram_address_start: *mut u8,

  // windows specific (bodies land in P2.2)
  #[cfg(target_os = "windows")]
  dolphin_proc_handle: *mut std::os::raw::c_void,
  #[cfg(target_os = "windows")]
  emu_ram_address_start: u64,
}

impl DolphinMemoryAccess {
  pub fn new() -> Self {
    Self {
      system: System::new(),
      attached_pid: -1,
      #[cfg(any(target_os = "linux", target_os = "macos"))]
      emu_ram_address_start: std::ptr::null_mut(),
      #[cfg(target_os = "windows")]
      dolphin_proc_handle: std::ptr::null_mut(),
      #[cfg(target_os = "windows")]
      emu_ram_address_start: 0,
    }
  }
}

impl Default for DolphinMemoryAccess {
  fn default() -> Self {
    Self::new()
  }
}

impl DolphinMemoryAccess {
  pub fn get_dolphin_pids(&mut self) -> Vec<Pid> {
    self.system.refresh_processes_specifics(
      ProcessesToUpdate::All,
      true,
      ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    self
      .system
      .processes()
      .iter()
      .filter_map(|(&pid, process)| {
        let exe = process.exe()?;
        let filename = exe.file_name()?;
        if filename == "dolphin-emu" {
          Some(pid)
        } else {
          None
        }
      })
      .collect()
  }

  /// Ports `MemoryAccess.cpp:73` (Linux) / `:392` (macOS) — identical bodies.
  pub fn attach_to_process(&mut self, pid: i32) -> bool {
    self.detach_from_process();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
      eprintln!("Dolphin found, PID {}", pid);
      let file_name = std::ffi::CString::new(format!("/dolphin-emu.{pid}"))
        .expect("shm name never contains an interior NUL");

      // SAFETY: `file_name` is a valid NUL-terminated C string that outlives the call.
      #[cfg(target_os = "linux")]
      let fd = unsafe { libc::shm_open(file_name.as_ptr(), libc::O_RDWR, 0o600) };
      // SAFETY: same as above; macOS `shm_open` is variadic so the mode is passed as c_int.
      #[cfg(target_os = "macos")]
      let fd = unsafe { libc::shm_open(file_name.as_ptr(), libc::O_RDWR, 0o600 as libc::c_int) };

      if fd < 0 {
        eprintln!(
          "Failed to open Dolphin shared memory: {}",
          std::io::Error::last_os_error()
        );
        return false;
      }

      // SAFETY: `fd` is a valid shm descriptor; requesting a fresh MAP_SHARED mapping of
      // DOLPHIN_SHM_SIZE bytes at a kernel-chosen address.
      let mem = unsafe {
        libc::mmap(
          std::ptr::null_mut(),
          DOLPHIN_SHM_SIZE,
          libc::PROT_READ | libc::PROT_WRITE,
          libc::MAP_SHARED,
          fd,
          0,
        )
      };

      // The fd is not needed once the mapping exists (C++ closes it on both paths).
      // SAFETY: `fd` is a valid descriptor we own and no longer use.
      unsafe { libc::close(fd) };

      if mem == libc::MAP_FAILED {
        eprintln!(
          "Failed to map Dolphin shared memory: {}",
          std::io::Error::last_os_error()
        );
        return false;
      }

      self.emu_ram_address_start = mem as *mut u8;
      self.attached_pid = pid;
      true
    }

    #[cfg(target_os = "windows")]
    {
      // P2.2: OpenProcess / VirtualQueryEx to locate the emulated RAM base.
      let _ = pid;
      false
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
      let _ = pid;
      false
    }
  }

  /// Ports `MemoryAccess.cpp:102` (Linux) / `:421` (macOS). Unlike the C++, a failed
  /// `munmap` logs and continues rather than `exit(4)`.
  pub fn detach_from_process(&mut self) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
      if !self.emu_ram_address_start.is_null() {
        eprintln!("Closing old shared memory");
        // SAFETY: `emu_ram_address_start` was returned by `mmap` with length
        // DOLPHIN_SHM_SIZE and has not been unmapped yet; we unmap it exactly once.
        let rc = unsafe {
          libc::munmap(
            self.emu_ram_address_start as *mut libc::c_void,
            DOLPHIN_SHM_SIZE,
          )
        };
        if rc != 0 {
          eprintln!(
            "Failed to unmap Dolphin shared memory: {}",
            std::io::Error::last_os_error()
          );
        }
        self.emu_ram_address_start = std::ptr::null_mut();
        self.attached_pid = -1;
      }
    }

    #[cfg(target_os = "windows")]
    {
      if !self.dolphin_proc_handle.is_null() {
        // P2.2: CloseHandle(self.dolphin_proc_handle) goes here.
        self.dolphin_proc_handle = std::ptr::null_mut();
        self.emu_ram_address_start = 0;
        self.attached_pid = -1;
      }
    }
  }

  pub fn get_attached_pid(&self) -> i32 {
    self.attached_pid
  }

  /// Ports `MemoryAccess.cpp:127` + `getRealPtr` (`:119`). Returns `false` and copies
  /// nothing when not attached or the offset is out of range (the C++ `getRealPtr`
  /// silently substitutes offset 0 — a latent bug; the only live caller passes 0).
  /// The copy is bounded by `dest.len()`, `size`, `DOLPHIN_MEMORY_SIZE`, and the
  /// mapping tail so a short `dest` can never be overrun.
  pub fn dolphin_memcpy(&self, dest: &mut [u8], offset: usize, size: usize) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
      if self.emu_ram_address_start.is_null() {
        return false;
      }
      let real_offset = offset & 0x7FFF_FFFF;
      if real_offset > DOLPHIN_MEMORY_SIZE {
        return false;
      }
      let n = size
        .min(DOLPHIN_MEMORY_SIZE)
        .min(dest.len())
        .min(DOLPHIN_SHM_SIZE - real_offset);
      // SAFETY: `real_offset <= DOLPHIN_MEMORY_SIZE < DOLPHIN_SHM_SIZE` and the mapping
      // spans DOLPHIN_SHM_SIZE bytes, so `emu_ram_address_start + real_offset` is in
      // bounds; `n` is clamped to both the mapping tail and `dest.len()`, and the two
      // regions cannot overlap (one is a private mmap, the other the caller's buffer).
      unsafe {
        std::ptr::copy_nonoverlapping(
          self.emu_ram_address_start.add(real_offset),
          dest.as_mut_ptr(),
          n,
        );
      }
      true
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
      // P2.2: Windows ReadProcessMemory path.
      let _ = (dest, offset, size);
      false
    }
  }
}

impl Drop for DolphinMemoryAccess {
  fn drop(&mut self) {
    self.detach_from_process();
  }
}
