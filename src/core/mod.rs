mod actions;
pub mod cli;
mod kill;
mod manager;
mod ps;

use crate::{
    gum::attach,
    util::{format::lengthed, highlight},
};
use actions::get_device;
use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};
use crossterm::style::Stylize;
use manager::Manager;
use std::process::exit;

pub fn execute_cli() {
    let cliparser = Cli::parse();
    // Handle completion generation if requested
    if let Some(shell) = cliparser.generate_completion {
        if let Err(e) = cliparser.generate_completion() {
            crate::util::logger::error(&format!("Failed to generate completion: {}", e));
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    let _manager = Manager::new();
    match &cliparser.command {
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let bin_name = "vlitz".to_string();
            clap_complete::generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
            std::process::exit(0);
        }
        Commands::Attach(args) => {
            let device_opt = get_device(&_manager, &args.connection);
            if let Some(mut device) = device_opt {
                attach(&mut device, &args.target);
                exit(0);
            } else {
                println!("{}", "No device found".red());
                exit(0);
            }
        }
        Commands::Ps(args) => {
            let device = get_device(&_manager, &args.connection);
            if let Some(device) = device {
                println!(
                    "{} {}",
                    "Device:".green(),
                    device.get_id().replace("\"", "").green()
                );
                let processes = ps::ps(&device, args);
                println!(
                    "{} {:<12} ({})",
                    lengthed("PID", 5).cyan().bold(),
                    "Process Name".yellow().bold(),
                    processes.len(),
                );
                for process in processes {
                    let process_name = if let Some(ref f) = args.filter {
                        highlight(process.get_name(), f)
                    } else {
                        process.get_name().to_string()
                    };
                    println!(
                        "{} {}",
                        lengthed(&process.get_pid().to_string(), 5).blue(),
                        process_name
                    );
                }
                exit(0);
            } else {
                println!("{}", "No device found".red());
                exit(0);
            }
        }
        Commands::Kill(args) => {
            let device = get_device(&_manager, &args.connection);
            if let Some(mut device) = device {
                let killed_processes = kill::kill(&mut device, &args.process);
                if killed_processes.is_empty() {
                    println!("No processes killed");
                } else {
                    for prc in killed_processes {
                        println!(
                            "Killed process {} {}",
                            format!("\"{}\"", prc.0).yellow(),
                            format!("[{}]", prc.1.to_string()).blue()
                        );
                    }
                    exit(0);
                }
            } else {
                println!("{}", "No device found".red());
                exit(0);
            }
        }
        Commands::Devices => {
            let devices = _manager.device_manager.enumerate_all_devices();
            println!(
                "{} {} {}",
                lengthed("Type", 6).cyan().bold(),
                lengthed("ID", 12).yellow().bold(),
                "Device Name".yellow().bold()
            );
            for device in devices {
                println!(
                    "{} {} {}",
                    lengthed(&device.get_type().to_string(), 6).blue(),
                    lengthed(device.get_id(), 12).white(),
                    device.get_name().grey()
                );
            }
            exit(0);
        }
    }
    exit(0);
}
