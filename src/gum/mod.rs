// src/gum/mod.rs
mod handler;
mod session;

pub mod commander;
pub mod filter;
pub mod list;
pub mod memory;
pub mod navigator;
pub mod store;
pub mod vzdata;

use std::process::exit;

use crate::core::cli::TargetArgs;
use crossterm::style::Stylize;
use frida::{Device, ScriptOption};
use handler::Handler;
use session::session_manager;

fn attach_pid<'a>(device: &'a Device, pid: u32) -> (frida::Session<'a>, u32) {
    (
        device.attach(pid).unwrap_or_else(|e| {
            println!(
                "{} {} ({})",
                "Failed to attach process:".red(),
                pid.to_string().yellow(),
                e
            );
            exit(0);
        }),
        pid,
    )
}

pub fn attach(device: &mut Device, args: &TargetArgs) {
    let (session, pid) = if let Some(_pid) = args.attach_pid {
        let pid: u32 = device
            .enumerate_processes()
            .iter()
            .find(|p| p.get_pid() == _pid)
            .map(|p| p.get_pid())
            .unwrap_or_else(|| {
                println!(
                    "{} {}",
                    "Process not found:".red(),
                    _pid.to_string().yellow()
                );
                exit(0);
            });
        attach_pid(device, pid)
    } else if let Some(ref file) = args.file {
        let pid = device
            .spawn(file, &frida::SpawnOptions::new())
            .unwrap_or_else(|e| {
                println!(
                    "{} {} ({})",
                    "Failed to spawn process:".red(),
                    file.to_string().yellow(),
                    e
                );
                exit(0);
            });
        attach_pid(device, pid)
    } else if let Some(ref name) = args.attach_name {
        let pid = device
            .enumerate_processes()
            .iter()
            .find(|p| p.get_name().to_lowercase() == name.to_lowercase())
            .map(|p| p.get_pid())
            .unwrap_or_else(|| {
                println!(
                    "{} {}",
                    "Process not found:".red(),
                    name.to_string().yellow()
                );
                exit(0);
            });
        attach_pid(device, pid)
    } else if let Some(ref name) = args.attach_identifier {
        let pid = device
            .enumerate_processes()
            .iter()
            .find(|p| p.get_name().to_lowercase() == name.to_lowercase())
            .map(|p| p.get_pid())
            .unwrap_or_else(|| {
                println!(
                    "{} {}",
                    "Process not found:".red(),
                    name.to_string().yellow()
                );
                exit(0);
            });
        attach_pid(device, pid)
    } else if let Some(ref name) = args.target {
        let pid = device
            .enumerate_processes()
            .iter()
            .find(|p| p.get_name().to_lowercase() == name.to_lowercase())
            .map(|p| p.get_pid())
            .unwrap_or_else(|| {
                println!(
                    "{} {}",
                    "Process not found:".red(),
                    name.to_string().yellow()
                );
                exit(0);
            });
        attach_pid(device, pid)
    } else {
        println!("{}", "No target specified".red());
        exit(0);
    };
    if session.is_detached() {
        println!("{}", "Session detached...".yellow().bold());
        return;
    }
    let script_content = include_str!("../agent.js").to_string();
    let mut script = session
        .create_script(&script_content, &mut ScriptOption::default())
        .unwrap_or_else(|e| {
            println!("{} {}", "Failed to create script:".red(), e);
            exit(0);
        });

    let handler = script.handle_message(Handler);
    if let Err(e) = handler {
        println!("{} {}", "Failed to set message handler:".red(), e);
        exit(0);
    }

    script.load().unwrap_or_else(|e| {
        println!("{} {}", "Failed to load script:".red(), e);
        exit(0);
    });

    if args.file.is_some() {
        device.resume(pid).unwrap_or_else(|e| {
            println!("{} {}", "Failed to resume process:".red(), e);
            exit(0);
        });
    }

    session_manager(&session, &mut script, pid);

    if !session.is_detached() {
        if let Err(e) = script.unload() {
            crate::util::logger::error(&format!("Failed to unload script: {}", e));
        }
        if let Err(e) = session.detach() {
            crate::util::logger::error(&format!("Failed to detach session: {}", e));
        } else {
            println!("{}", "Session detached.".yellow().bold());
        }
    }
}
