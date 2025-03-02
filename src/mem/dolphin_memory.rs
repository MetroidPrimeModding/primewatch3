use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub const DOLPHIN_MEMORY_SIZE: usize = 0x1800000;

pub struct DolphinMemoryAccess {
  system: System,
  attached_pid: i32,

  // linux specific
  #[cfg(target_os = "linux")]
  emu_ram_address_start: *mut u8,

  // windows specific
  #[cfg(target_os = "windows")]
  dolphin_proc_handle: *mut std::os::raw::c_void,
  #[cfg(target_os = "windows")]
  emu_ram_address_start: u64,

  // macos specific
  #[cfg(target_os = "macos")]
  emu_ram_address_start: *mut u8,
}

impl DolphinMemoryAccess {
  pub fn new() -> Self {
    Self {
      system: System::new(),
      attached_pid: -1,
      #[cfg(target_os = "linux")]
      emu_ram_address_start: std::ptr::null_mut(),
      #[cfg(target_os = "windows")]
      dolphin_proc_handle: std::ptr::null_mut(),
      #[cfg(target_os = "windows")]
      emu_ram_address_start: 0,
      #[cfg(target_os = "macos")]
      emu_ram_address_start: std::ptr::null_mut(),
    }
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
        let Some(exe) = process.exe() else {
          return None;
        };
        let Some(filename) = exe.file_name() else {
          return None;
        };
        if filename == "dolphin-emu" {
          Some(pid)
        } else {
          None
        }
      })
      .collect()
  }

  pub fn attach_to_process(&mut self, pid: i32) -> bool {
    self.detach_from_process();
    eprintln!("Dolphin found, PID {}", pid);
    self.attached_pid = pid;

    #[cfg(target_os = "linux")]
    {
      let file_name = format!("/dolphin-emu.{}", pid);
      // For demonstration, we omit the full shm_open & mmap steps in Rust
      // Here you would actually open and map shared memory
      todo!("Open and map shared memory");
      // if successful:
      true
    }

    #[cfg(target_os = "windows")]
    {
      eprintln!("Connecting to Dolphin pid {}", pid);
      todo!("OpenProcess");
      true
    }

    #[cfg(target_os = "macos")]
    {
      // TODO: open/map shared memory
      true
    }
  }

  pub fn detach_from_process(&mut self) {
    #[cfg(target_os = "linux")]
    {
      if !self.emu_ram_address_start.is_null() {
        eprintln!("Closing old shared memory");
        // munmap here
        self.emu_ram_address_start = std::ptr::null_mut();
        self.attached_pid = -1;
      }
    }

    #[cfg(target_os = "windows")]
    {
      if !self.dolphin_proc_handle.is_null() {
        eprintln!("Closing process handle");
        todo!("CloseHandle");
        self.dolphin_proc_handle = std::ptr::null_mut();
        self.emu_ram_address_start = 0;
        self.attached_pid = -1;
      }
    }

    #[cfg(target_os = "macos")]
    {
      if !self.emu_ram_address_start.is_null() {
        self.emu_ram_address_start = std::ptr::null_mut();
        self.attached_pid = -1;
      }
    }
  }

  pub fn get_attached_pid(&self) -> i32 {
    self.attached_pid
  }

  pub fn dolphin_memcpy(&self, dest: &mut [u8], offset: usize, size: usize) -> bool {
    #[cfg(target_os = "linux")]
    {
      // todo: verify this, ai wrote it
      if self.emu_ram_address_start.is_null() {
        return false;
      }
      let copy_size = size.min(DOLPHIN_MEMORY_SIZE);
      let src_ptr = unsafe {
        self
          .emu_ram_address_start
          .add((offset & 0x7FFF_FFFF).min(DOLPHIN_MEMORY_SIZE as usize))
      };
      unsafe {
        std::ptr::copy_nonoverlapping(src_ptr, dest.as_mut_ptr(), copy_size);
      }
    }

    #[cfg(target_os = "windows")]
    {
      todo!("ReadProcessMemory");
    }

    #[cfg(target_os = "macos")]
    {
      // Demonstration only
      false
    }
  }
}

pub use DolphinMemoryAccess as MemoryAccessImpl;
