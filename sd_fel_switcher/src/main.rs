#![no_std]
#![no_main]

// eGON.BT0 header + BROM entry (must live in .text._start, see link.ld)
core::arch::global_asm!(
  r#"
    .option norvc
    .section .text._start, "ax", @progbits
    .global _start
_start:
    jal     x0, _reset
    .byte   'e','G','O','N'
    .byte   '.','B','T','0'
    .word   0x5F0A6C39
    .word   0x00004000
    .word   0x00000030
    .byte   '3','0','0','0'
    .word   0x00020000
    .word   0x00020000
    .word   0x00000000
    .byte   0x00, 0x00, 0x00, 0x00
    .byte   '4', '.', '0', 0x00

    .section .text, "ax", @progbits
    .global _reset
_reset:
    li      sp, 0x00028000
    tail    main
"#
);

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

const CCU_AHB1_CFG: usize = 0x0200_1510;
const CCU_APB1_CFG: usize = 0x0200_1520;
const CCU_CPUX_AXI: usize = 0x0200_1D00;
const CCU_USB_BGR: usize = 0x0200_1A8C;

const R_AHB_BUS_RTC: usize = 0x0701_020C;
const RTC_FEL: usize = 0x0709_0108;
const EFEX_FLAG: u32 = 0x5AA5_A55A;

const SYS_RST: usize = 0x0205_00A8;
const SYS_RST_VAL: u32 = 0x16AA_0001;

const BROM_FEL: u32 = 0x20;

#[inline(always)]
unsafe fn reg32(addr: usize) -> *mut u32 {
  addr as *mut u32
}

#[inline(always)]
unsafe fn read32(addr: usize) -> u32 {
  read_volatile(reg32(addr))
}

#[inline(always)]
unsafe fn write32(addr: usize, val: u32) {
  write_volatile(reg32(addr), val);
  compiler_fence(Ordering::SeqCst);
}

fn delay_ms(ms: u32) {
  for _ in 0..ms {
    for _ in 0..24_000 {
      core::hint::spin_loop();
    }
  }
}

fn board_clock_reset() {
  unsafe {
    let mut v = read32(CCU_AHB1_CFG);
    v &= !((0x3 << 24) | (0x3 << 8) | 0x3);
    write32(CCU_AHB1_CFG, v);

    v = read32(CCU_APB1_CFG);
    v &= !((0x3 << 24) | (0x3 << 8) | 0x3);
    write32(CCU_APB1_CFG, v);

    write32(CCU_CPUX_AXI, 0x0301);
  }
}

fn usb0_clock_on() {
  unsafe {
    let mut v = read32(CCU_USB_BGR);
    v |= 1 << 16;
    write32(CCU_USB_BGR, v);
  }
  delay_ms(1);
  unsafe {
    let mut v = read32(CCU_USB_BGR);
    v |= 1 << 0;
    write32(CCU_USB_BGR, v);
  }
}

fn rtc_clear_fel_flag() {
  loop {
    unsafe {
      write32(RTC_FEL, 0);
      if read32(RTC_FEL) == 0 {
        break;
      }
    }
  }
}

#[inline(never)]
fn boot0_jmp_fel(addr: u32) -> ! {
  unsafe {
    asm!(
      "mv a0, {0}",
      "jr a0",
      in(reg) addr,
      options(noreturn),
    );
  }
}

fn enter_fel() -> ! {
  board_clock_reset();
  usb0_clock_on();
  delay_ms(10);
  boot0_jmp_fel(BROM_FEL);
}

fn system_reset() -> ! {
  unsafe {
    write32(SYS_RST, SYS_RST_VAL);
  }
  loop {
    unsafe {
      asm!("wfi", options(nomem, nostack));
    }
  }
}

#[no_mangle]
pub extern "C" fn main() -> ! {
  unsafe {
    let mut v = read32(R_AHB_BUS_RTC);
    v |= (1 << 16) | (1 << 0);
    write32(R_AHB_BUS_RTC, v);
  }

  if unsafe { read32(RTC_FEL) } == EFEX_FLAG {
    rtc_clear_fel_flag();
    enter_fel();
  }

  enter_fel();

  #[allow(unreachable_code)]
  loop {
    unsafe {
      write32(RTC_FEL, EFEX_FLAG);
      if read32(RTC_FEL) == EFEX_FLAG {
        break;
      }
    }
  }

  system_reset();
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
  loop {
    core::hint::spin_loop();
  }
}
