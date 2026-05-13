use std::env;
use std::path::PathBuf;

// 32 KiB recommended by [nrf-modem](https://github.com/diondokter/nrf-modem?tab=readme-ov-file#memory)
#[allow(dead_code, reason = "only used when the feature is enabled")]
const NRF91_MODEM_IPC_KB: u64 = 32;

fn parse_dec_or_hex(input: String) -> Result<u64, std::num::ParseIntError> {
    if let Some(hex) = input.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        input.parse::<u64>()
    }
}

fn main() {
    if !context("ariel-os") {
        // Platform-independent tooling.
        return;
    }

    // Put the linker scripts somewhere the linker can find them
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());

    if let Some(context) = context_any(&["esp32c3", "cortex-m", "riscv"]) {
        let insert_somewhere = match context {
            "esp32c3" => "INSERT AFTER .rwdata_dummy;",
            "cortex-m" => "INSERT BEFORE .data;",
            "riscv" => "INSERT BEFORE .trap;",
            _ => "",
        };

        let region = match context {
            "cortex-m" => "RAM",
            "riscv" | "esp32c3" => "RWDATA",
            _ => unreachable!(),
        };

        let mut isr_stack_template = std::fs::read_to_string("isr_stack.ld.in").unwrap();
        isr_stack_template = isr_stack_template.replace("${INSERT_SOMEWHERE}", insert_somewhere);
        isr_stack_template = isr_stack_template.replace("${STACK_REGION}", region);
        std::fs::write(out.join("isr_stack.x"), &isr_stack_template).unwrap();
        println!("cargo:rerun-if-changed=isr_stack.ld.in");
    }

    if context("riscv") {
        let region_alias = if context("esp32c3") {
            "REGION_ALIAS(FLASH, DROM)"
        } else if context("esp32c6") {
            "REGION_ALIAS(FLASH, ROM)"
        } else {
            panic!("unexpected riscv platform");
        };
        std::fs::write(out.join("linkme-region-alias.x"), region_alias).unwrap();
    }

    if context("xtensa") {
        let isr_stacksize =
            std::env::var("CONFIG_ISR_STACKSIZE").expect("CONFIG_ISR_STACKSIZE env var not set");
        let template = std::fs::read_to_string("isr_stack_xtensa.ld.in")
            .unwrap()
            .replace("${ISR_STACKSIZE}", &isr_stacksize);
        std::fs::write(out.join("isr_stack_xtensa.x"), &template).unwrap();
        println!("cargo:rerun-if-changed=isr_stack_xtensa.ld.in");
    }

    std::fs::copy("linkme.x", out.join("linkme.x")).unwrap();
    std::fs::copy("eheap.x", out.join("eheap.x")).unwrap();
    std::fs::copy("keep-stack-sizes.x", out.join("keep-stack-sizes.x")).unwrap();

    #[cfg(feature = "memory-x")]
    write_memoryx();

    println!("cargo:rerun-if-changed=linkme.x");
    println!("cargo:rerun-if-changed=eheap.x");
    println!("cargo:rerun-if-changed=keep-stack-sizes.x");

    println!("cargo:rustc-link-search={}", out.display());
}

/// Writes `memory.x` based on `ld-memory` settings to `$OUTDIR`.
///
/// # Panics
/// Panics if called outside of a known laze context.
#[cfg(feature = "memory-x")]
fn write_memoryx() {
    use ld_memory::{Memory, MemorySection};
    let rom_start = parse_dec_or_hex(
        std::env::var("CHIP_ROM_START_ADDRESS").expect("CHIP_ROM_START_ADDRESS env var not set"),
    )
    .expect("CHIP_ROM_START_ADDRESS is not a decimal or hex value");
    let rom_page_size = parse_dec_or_hex(
        std::env::var("CHIP_ROM_PAGE_SIZE_BYTES")
            .expect("CHIP_ROM_PAGE_SIZE_BYTES env var not set"),
    )
    .expect("CHIP_ROM_PAGE_SIZE_BYTES is not a decimal or hex value");
    let rom_page_count = std::env::var("CHIP_ROM_PAGE_COUNT")
        .expect("CHIP_ROM_PAGE_COUNT env var not set")
        .parse::<u64>()
        .expect("CHIP_ROM_PAGE_COUNT is not a decimal number");

    let (ram, flash) = if context("nrf51822-xxaa") {
        (16, 256)
    } else if context("nrf52832") {
        (64, 256)
    } else if context("nrf52833") {
        (128, 512)
    } else if context("nrf52840") {
        (256, 1024)
    } else if context("nrf5340-app") {
        (512, 1024)
    } else if context("nrf5340-net") {
        (64, 256)
    } else if context_any(&["nrf9151", "nrf9160"]).is_some() {
        let ram = 256;
        let flash = 1024;
        if cfg!(feature = "nrf91-modem") {
            (ram - NRF91_MODEM_IPC_KB, flash)
        } else {
            (ram, flash)
        }
    } else {
        panic!("please set the MCU laze context");
    };

    let (pagesize, ram_base, flash_base) = if context("nrf5340-net") {
        (2048, 0x2100_0000, 0x0100_0000)
    } else if cfg!(feature = "nrf91-modem") {
        (4096, 0x2000_0000 + NRF91_MODEM_IPC_KB * 1024, 0)
    } else {
        (4096, 0x2000_0000, 0x0)
    };

    let chip =
        rommel::chip::Chip::new_bytes(rom_page_size, rom_start, rom_page_size * rom_page_count)
            .unwrap();
    let mut flash = rommel::Memory::new(chip);
    context_to_linker(&mut flash).expect("Unable to construct layout");

    // generate linker script
    let memory = flash
        .resolve_layout()
        .unwrap()
        .into_memory()
        .add_section(MemorySection::new("RAM", ram_base, ram * 1024));

    #[cfg(feature = "nrf91-modem")]
    let memory = memory.add_section(MemorySection::new(
        "MODEM",
        0x2000_0000,
        NRF91_MODEM_IPC_KB * 1024,
    ));

    memory.to_cargo_outdir("memory.x").expect("wrote memory.x");
}

fn context_to_linker(flash: &mut rommel::Memory) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "CARGO_CFG_CONTEXT: {:?}",
        std::env::var("CARGO_CFG_CONTEXT")
    );
    let app = rommel::section::Section::new("FLASH")
        .unwrap()
        .set_maximize(true);

    #[cfg(feature = "embassy-boot")]
    {
        flash.add_section(app);
        let layout = load_layout("embassy-boot.yaml")?;
        flash.layout_mut().merge(layout);
    }
    #[cfg(not(feature = "embassy-boot"))]
    flash.add_section(app.set_boot(true));
    Ok(())
}

fn load_layout(target: &str) -> Result<rommel::layout::Layout, Box<dyn std::error::Error>> {
    let path = std::env::var("ROMMEL_SNIPPET_DIR").expect("Linker snippet directory not set");
    let target_path = std::path::Path::new(&path).join(target);
    let reader = std::fs::File::open(&target_path)?;
    let sections: rommel::layout::Layout = yaml_serde::from_reader(reader)?;
    println!("cargo:rerun-if-changed={}", target_path.display());
    Ok(sections)
}

/// Returns the first of the given contexts that is in the current `cfg` contexts.
fn context_any(contexts: &[&'static str]) -> Option<&'static str> {
    // Contexts cannot include commas.
    contexts.iter().find(|c| context(c)).copied()
}

/// Returns whether the given context is in the current 'cfg' contexts.
fn context(context: &'static str) -> bool {
    let Ok(context_var) = std::env::var("CARGO_CFG_CONTEXT") else {
        return false;
    };

    // Contexts cannot include commas.
    context_var.split(',').any(|c| c == context)
}
