#![no_std]
#![no_main]
#![feature(asm)]
#![feature(const_fn)]
#![feature(core_intrinsics)]
#![feature(prelude_import)]
#![feature(try_trait)]
#![allow(non_snake_case)]

#[macro_use]
extern crate bitflags;
extern crate coreboot_table;
extern crate dmi;
extern crate ecflash;
#[macro_use]
extern crate memoffset;
extern crate orbclient;
extern crate orbfont;
extern crate plain;
extern crate spin;
#[macro_use]
extern crate uefi_std as std;
extern crate rlibc;

#[allow(unused_imports)]
#[prelude_import]
use std::prelude::*;

use core::ptr;
use uefi::status::Status;

#[macro_use]
mod debug;

// Public to ignore unused functions
pub mod image;
pub mod serial;

mod coreboot;
mod display;
//mod dump_hii;
mod fde;
mod hii;
mod key;
mod rng;
mod security;
mod ui;

#[no_mangle]
pub extern "C" fn main() -> Status {
    let uefi = std::system_table();

    let _ = (uefi.BootServices.SetWatchdogTimer)(0, 0, 0, ptr::null());

    coreboot::init();

    if let Err(err) = fde::Fde::install() {
        println!("Fde error: {:?}", err);
        let _ = key::key(true);
    }

    if let Err(err) = security::install() {
        debugln!("security error: {:?}", err);
        let _ = key::key(true);
    }

    Status(0)
}
