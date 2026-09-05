use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Lowercased `exe` file-stem prefix identifying a Dolphin process.
/// Linux/macOS: the emulator binary is `dolphin-emu` (also `dolphin-emu-nogui`,
/// `dolphin-emu-qt2`, …) — must NOT be just `dolphin`, which is KDE's file manager.
/// Windows: the binary is `Dolphin.exe`, whose stem is `dolphin`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const DOLPHIN_STEM_PREFIX: &str = "dolphin-emu";
#[cfg(target_os = "windows")]
const DOLPHIN_STEM_PREFIX: &str = "dolphin";

/// Copy cap: the amount of emulated RAM (MEM1) the game snapshot mirrors.
pub const DOLPHIN_MEMORY_SIZE: usize = 0x1800000;

/// The span of Dolphin's shared-memory mapping (`mmap`/`munmap` length).
/// Larger than `DOLPHIN_MEMORY_SIZE` because it also covers the L1 cache / MEM2 region.
const DOLPHIN_SHM_SIZE: usize = 0x2040000;

pub struct DolphinMemoryAccess {
  system: System,
  attached_pid: i32,

  // POSIX (Linux + macOS share the shm_open/mmap path).
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  emu_ram_address_start: *mut u8,

  // Windows: raw `HANDLE` from `OpenProcess` (wrapped as `windows::…::HANDLE` at
  // call sites) plus the remote base address of Dolphin's MEM1 mapping.
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
        // On Linux, sysinfo lists every thread (`/proc/<pid>/task/<tid>`) as its
        // own `Process` sharing the parent's `exe()`. Those have a `thread_kind`;
        // real processes have `None`. Skip threads so we return one PID per
        // Dolphin instance, not one per Dolphin thread.
        if process.thread_kind().is_some() {
          return None;
        }
        let exe = process.exe()?;
        let stem = exe.file_stem()?.to_str()?.to_ascii_lowercase();

        if stem.starts_with(DOLPHIN_STEM_PREFIX) {
          Some(pid)
        } else {
          None
        }
      })
      .collect()
  }

  /// Scans the target's virtual address space for Dolphin's MEM1 mapping: the
  /// first `0x2000000`-byte `MEM_MAPPED` region that `QueryWorkingSetEx` reports
  /// as physically valid (this disambiguates unrelated mapped regions of the
  /// same size). Stores its base in `self.emu_ram_address_start` and returns
  /// `true` on success.
  ///
  /// Deviations from C++:
  /// - The original keeps scanning past MEM1 to set the `MEM2Present` flag; that
  ///   flag is never read anywhere by the app, so we stop at the first valid
  ///   MEM1 region and do not carry `MEM2Present`.
  /// - `wsInfo.VirtualAttributes.Valid` is bit 0 of the bitfield union; this
  ///   `windows` crate version generates no `.Valid()` accessor, so we mask
  ///   `Flags & 1`.
  #[cfg(target_os = "windows")]
  fn get_emu_ram_address_start(&mut self) -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Memory::{MEM_MAPPED, MEMORY_BASIC_INFORMATION, VirtualQueryEx};
    use windows::Win32::System::ProcessStatus::{
      PSAPI_WORKING_SET_EX_INFORMATION, QueryWorkingSetEx,
    };

    if self.dolphin_proc_handle.is_null() {
      return false;
    }
    let handle = HANDLE(self.dolphin_proc_handle);
    let info_size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();

    let mut addr: usize = 0;
    loop {
      let mut info = MEMORY_BASIC_INFORMATION::default();
      // SAFETY: `handle` is a live process handle opened with PROCESS_QUERY_INFORMATION;
      // `info` is a valid, correctly sized, zeroed output buffer; `addr` is a plain
      // integer address, never dereferenced by us.
      let written = unsafe {
        VirtualQueryEx(
          handle,
          Some(addr as *const core::ffi::c_void),
          &mut info,
          info_size,
        )
      };
      if written != info_size || info.RegionSize == 0 {
        break;
      }

      if info.RegionSize == 0x2000000 && info.Type == MEM_MAPPED {
        let mut ws = PSAPI_WORKING_SET_EX_INFORMATION {
          VirtualAddress: info.BaseAddress,
          ..Default::default()
        };
        // SAFETY: `handle` is live; `ws` is a valid, correctly sized buffer whose
        // `VirtualAddress` we just set to the region base being queried.
        let ok = unsafe {
          QueryWorkingSetEx(
            handle,
            (&raw mut ws).cast(),
            std::mem::size_of::<PSAPI_WORKING_SET_EX_INFORMATION>() as u32,
          )
        }
        .is_ok();
        // SAFETY: reading the `Flags` arm of a `#[repr(C)]` union whose members are
        // all `usize`-sized is always valid.
        let valid = ok && (unsafe { ws.VirtualAttributes.Flags } & 1) != 0;
        if valid {
          self.emu_ram_address_start = info.BaseAddress as u64;
          println!("Found ram start: {:#x}", self.emu_ram_address_start);
          return true;
        }
      }

      match addr.checked_add(info.RegionSize) {
        Some(next) => addr = next,
        None => break,
      }
    }

    false
  }

  /// Attach to the Dolphin process `pid`.
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

      // The fd is not needed once the mapping exists.
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
      use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
      };

      println!("Connecting to Dolphin pid {pid}");

      // SAFETY: FFI call; `pid` is passed by value and the access-rights flags are
      // plain bitmasks — no pointers are involved.
      let handle = match unsafe {
        OpenProcess(
          PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_READ,
          false,
          pid as u32,
        )
      } {
        Ok(h) => h,
        Err(e) => {
          eprintln!("Failed to open Dolphin process {pid}: {e}");
          return false;
        }
      };

      self.dolphin_proc_handle = handle.0;

      if !self.get_emu_ram_address_start() {
        // Wait for Dolphin to start running a game.
        println!("Detected dolphin isn't running a game. We'll check for it in copy.");
      }

      self.attached_pid = pid;
      true
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
      let _ = pid;
      false
    }
  }

  /// Detach from the attached Dolphin process.
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
        println!("Closing process");
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        // SAFETY: `dolphin_proc_handle` is a handle we obtained from `OpenProcess`
        // and have not yet closed; the `is_null` guard ensures we close it once.
        let _ = unsafe { CloseHandle(HANDLE(self.dolphin_proc_handle)) };
        self.dolphin_proc_handle = std::ptr::null_mut();
        self.emu_ram_address_start = 0;
        self.attached_pid = -1;
      }
    }
  }

  pub fn get_attached_pid(&self) -> i32 {
    self.attached_pid
  }

  /// Whether the currently attached process is still running. `false` when
  /// nothing is attached. Lets callers notice a Dolphin process that exited
  /// on its own (a "natural" disconnect) as opposed to an explicit
  /// `detach_from_process` call.
  pub fn is_attached_process_alive(&mut self) -> bool {
    if self.attached_pid <= 0 {
      return false;
    }
    let pid = Pid::from_u32(self.attached_pid as u32);
    self.system.refresh_processes_specifics(
      ProcessesToUpdate::Some(&[pid]),
      true,
      ProcessRefreshKind::nothing(),
    );
    self.system.process(pid).is_some()
  }

  /// Returns `false` and copies nothing when not attached or the offset is out
  /// of range. The copy is bounded by `dest.len()`, `size`, `DOLPHIN_MEMORY_SIZE`,
  /// and the mapping tail so a short `dest` can never be overrun.
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

    #[cfg(target_os = "windows")]
    {
      use windows::Win32::Foundation::{ERROR_PARTIAL_COPY, GetLastError, HANDLE};
      use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;

      if self.dolphin_proc_handle.is_null() || self.emu_ram_address_start == 0 {
        // Base not resolved yet (Dolphin running, no game). The caller
        // re-attaches every frame, which re-runs the region scan. Keeping the
        // `&self` signature means we cannot re-scan in place here.
        return false;
      }

      let real_offset = offset & 0x7FFF_FFFF;
      if real_offset > DOLPHIN_MEMORY_SIZE {
        return false;
      }

      // Clamp to `dest.len()` as well as `DOLPHIN_MEMORY_SIZE`
      let n = size.min(DOLPHIN_MEMORY_SIZE).min(dest.len());
      let mut read: usize = 0;
      // SAFETY: `dolphin_proc_handle` is a live handle with PROCESS_VM_READ;
      // `emu_ram_address_start + real_offset` lies inside the 0x2000000-byte MEM1
      // mapping (`real_offset <= DOLPHIN_MEMORY_SIZE`); `dest` has room for `n` bytes
      // (clamped to `dest.len()`); `read` is a valid out pointer. The remote address
      // is only read by the kernel, never dereferenced in this process.
      let result = unsafe {
        ReadProcessMemory(
          HANDLE(self.dolphin_proc_handle),
          (self.emu_ram_address_start + real_offset as u64) as *const core::ffi::c_void,
          dest.as_mut_ptr().cast(),
          n,
          Some(&mut read),
        )
      };

      if let Err(e) = result {
        // SAFETY: FFI call with no arguments.
        let err = unsafe { GetLastError() };
        eprintln!(
          "Failed to read memory from {offset:#x}. Error: {} ({e})",
          err.0
        );
        if err == ERROR_PARTIAL_COPY {
          eprintln!("Game probably closed. Will continue looking.");
        }
        return false;
      }

      if read != n {
        // Warn but do not fail hard
        eprintln!("Failed to read enough from {offset:#x}. Read {read} of {n}");
      }
      read == n
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
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
