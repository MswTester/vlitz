// src/gum/commander.rs
use crate::gum::{
    filter::parse_filter_string,
    list::{list_functions, list_ranges, list_variables},
};
use crate::util::log_error;
use crossterm::style::Stylize;

use super::{list::list_modules, navigator::Navigator, store::Store, vzdata::VzData};
use frida::Script;
use regex::Regex;
use std::{fmt, vec};

#[derive(Debug)]
struct CommandArg {
    name: String,
    description: String,
    required: bool,
}

impl CommandArg {
    fn new(name: &str, description: &str, required: bool) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            required,
        }
    }

    fn required(name: &str, description: &str) -> Self {
        Self::new(name, description, true)
    }

    fn optional(name: &str, description: &str) -> Self {
        Self::new(name, description, false)
    }
}

impl fmt::Display for CommandArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.required {
            write!(f, "<{}>", self.name)
        } else {
            write!(f, "[{}]", self.name)
        }
    }
}

type CommandHandler = fn(&mut Commander, &[&str]) -> bool;

struct SubCommand {
    name: String,
    aliases: Vec<String>,
    description: String,
    args: Vec<CommandArg>,
    execute: CommandHandler,
}

impl SubCommand {
    fn new(name: &str, description: &str, args: Vec<CommandArg>, execute: CommandHandler) -> Self {
        Self {
            name: name.to_string(),
            aliases: Vec::new(),
            description: description.to_string(),
            args,
            execute,
        }
    }

    fn alias(mut self, alias: &str) -> Self {
        self.aliases.push(alias.to_string());
        self
    }
}

struct Command {
    command: String,
    description: String,
    aliases: Vec<String>,
    args: Vec<CommandArg>,
    subcommands: Vec<SubCommand>,
    default_execute: Option<CommandHandler>,
}

impl Command {
    fn new(
        command: &str,
        description: &str,
        aliases: Vec<&str>,
        args: Vec<CommandArg>,
        subcommands: Vec<SubCommand>,
        default_execute: Option<CommandHandler>,
    ) -> Self {
        Self {
            command: command.to_string(),
            description: description.to_string(),
            aliases: aliases.into_iter().map(String::from).collect(),
            args,
            subcommands,
            default_execute,
        }
    }
}

pub struct Commander<'a, 'b> {
    script: &'a mut Script<'b>,
    pub env: String,
    field: Store,
    lib: Store,
    pub navigator: Navigator,
    commands: Vec<Command>,
}

impl<'a, 'b> Commander<'a, 'b> {
    pub fn new(script: &'a mut Script<'b>) -> Self {
        let env_value = script
            .exports
            .call("get_env", None)
            .expect("Failed to call get_env")
            .expect("Failed to get env value");
        let env_arr = env_value.as_array().cloned().unwrap_or_default();
        let os = env_arr.get(0).and_then(|v| v.as_str()).unwrap_or("");
        let arch = env_arr.get(1).and_then(|v| v.as_str()).unwrap_or("");
        Commander {
            script,
            env: format!("{} {}", os, arch),
            field: Store::new("Field".to_string()),
            lib: Store::new("Lib".to_string()),
            navigator: Navigator::new(),
            commands: vec![
                Command::new(
                    "debug",
                    "Debug functions",
                    vec!["d"],
                    vec![],
                    vec![SubCommand::new("exports", "List exports", vec![], |c, a| {
                        Commander::debug_exports(c, a)
                    })
                    .alias("e")],
                    None,
                ),
                Command::new(
                    "help",
                    "Show this help message",
                    vec!["h"],
                    vec![CommandArg::optional("command", "Command to show help for")],
                    vec![],
                    Some(|c, a| Commander::help(c, a)),
                ),
                Command::new(
                    "exit",
                    "Exit the session",
                    vec!["quit", "q"],
                    vec![],
                    vec![],
                    Some(|c, a| Commander::exit(c, a)),
                ),
                Command::new(
                    "select",
                    "Select data",
                    vec!["sel", "sl"],
                    vec![CommandArg::required("selector", "Selector")],
                    vec![],
                    Some(|c, a| Commander::select(c, a)),
                ),
                Command::new(
                    "deselect",
                    "Deselect data",
                    vec!["desel", "dsl"],
                    vec![],
                    vec![],
                    Some(|c, a| Commander::deselect(c, a)),
                ),
                Command::new(
                    "add",
                    "Add offset to selected data",
                    vec!["+"],
                    vec![CommandArg::required("offset", "Offset")],
                    vec![],
                    Some(|c, a| Commander::add(c, a)),
                ),
                Command::new(
                    "sub",
                    "Subtract offset from selected data",
                    vec!["-"],
                    vec![CommandArg::required("offset", "Offset")],
                    vec![],
                    Some(|c, a| Commander::sub(c, a)),
                ),
                Command::new(
                    "goto",
                    "Go to address",
                    vec!["go", ":"],
                    vec![CommandArg::required("address", "Address")],
                    vec![],
                    Some(|c, a| Commander::goto(c, a)),
                ),
                Command::new(
                    "field",
                    "Field manipulation commands.",
                    vec!["fld", "f"],
                    vec![CommandArg::optional("page", "Page number")],
                    vec![
                        SubCommand::new(
                            "list",
                            "List fields with optional page number",
                            vec![CommandArg::optional("page", "Page number")],
                            |c, a| Commander::field_list(c, a),
                        )
                        .alias("ls")
                        .alias("l"),
                        SubCommand::new(
                            "next",
                            "Go to next page of fields",
                            vec![CommandArg::optional("page", "Page number")],
                            |c, a| Commander::field_next(c, a),
                        )
                        .alias("n"),
                        SubCommand::new(
                            "prev",
                            "Go to previous page of fields",
                            vec![CommandArg::optional("page", "Page number")],
                            |c, a| Commander::field_prev(c, a),
                        )
                        .alias("p"),
                        SubCommand::new(
                            "sort",
                            "Sort fields by name",
                            vec![CommandArg::optional("type", "Sort type [addr]")],
                            |c, a| Commander::field_sort(c, a),
                        )
                        .alias("s"),
                        SubCommand::new(
                            "move",
                            "Move fields",
                            vec![
                                CommandArg::required("from", "Index of data"),
                                CommandArg::required("to", "Index of data"),
                            ],
                            |c, a| Commander::field_move(c, a),
                        )
                        .alias("mv"),
                        SubCommand::new(
                            "remove",
                            "Remove data from field",
                            vec![
                                CommandArg::required("index", "Index of data"),
                                CommandArg::optional(
                                    "count",
                                    "Count of data to remove (default: 1)",
                                ),
                            ],
                            |c, a| Commander::field_remove(c, a),
                        )
                        .alias("rm")
                        .alias("del")
                        .alias("delete"),
                        SubCommand::new("clear", "Clear all fields", vec![], |c, a| {
                            Commander::field_clear(c, a)
                        })
                        .alias("cls")
                        .alias("clr")
                        .alias("cl")
                        .alias("c"),
                        SubCommand::new(
                            "filter",
                            "Filter fields",
                            vec![CommandArg::required(
                                "filter",
                                "Filter as filter expression",
                            )],
                            |c, a| Commander::field_filter(c, a),
                        )
                        .alias("f")
                        .alias("filter"),
                    ],
                    Some(|c, a| Commander::field_list(c, a)),
                ),
                Command::new(
                    "save",
                    "Save data from field",
                    vec!["sv"],
                    vec![CommandArg::required("selector", "Selector from field")],
                    vec![],
                    Some(|c, a| Commander::lib_save(c, a)),
                ),
                Command::new(
                    "lib",
                    "Library manipulation commands.",
                    vec!["lb", "l"],
                    vec![CommandArg::optional("page", "Page number")],
                    vec![
                        SubCommand::new(
                            "list",
                            "List libraries with optional page number",
                            vec![CommandArg::optional("page", "Page number")],
                            |c, a| Commander::lib_list(c, a),
                        )
                        .alias("ls")
                        .alias("l"),
                        SubCommand::new(
                            "next",
                            "Go to next page of libraries",
                            vec![CommandArg::optional("page", "Page number")],
                            |c, a| Commander::lib_next(c, a),
                        )
                        .alias("n"),
                        SubCommand::new(
                            "prev",
                            "Go to previous page of libraries",
                            vec![CommandArg::optional("page", "Page number")],
                            |c, a| Commander::lib_prev(c, a),
                        )
                        .alias("p"),
                        SubCommand::new(
                            "sort",
                            "Sort libraries by name",
                            vec![CommandArg::optional("type", "Sort type [addr]")],
                            |c, a| Commander::lib_sort(c, a),
                        )
                        .alias("s"),
                        SubCommand::new(
                            "move",
                            "Move data from one library to another",
                            vec![
                                CommandArg::required("from", "Index of data"),
                                CommandArg::required("to", "Index of data"),
                            ],
                            |c, a| Commander::lib_move(c, a),
                        )
                        .alias("mv"),
                        SubCommand::new(
                            "remove",
                            "Remove data from library",
                            vec![
                                CommandArg::required("index", "Index of data"),
                                CommandArg::optional(
                                    "count",
                                    "Count of data to remove (default: 1)",
                                ),
                            ],
                            |c, a| Commander::lib_remove(c, a),
                        )
                        .alias("rm")
                        .alias("del")
                        .alias("delete"),
                        SubCommand::new("clear", "Clear all data", vec![], |c, a| {
                            Commander::lib_clear(c, a)
                        })
                        .alias("cls")
                        .alias("clr")
                        .alias("cl")
                        .alias("c"),
                        SubCommand::new(
                            "filter",
                            "Filter libraries",
                            vec![CommandArg::required("filter", "Filter expression")],
                            |c, a| Commander::lib_filter(c, a),
                        )
                        .alias("f")
                        .alias("filter"),
                    ],
                    Some(|c, a| Commander::lib_list(c, a)),
                ),
                Command::new(
                    "list",
                    "List all data",
                    vec!["ls"],
                    vec![],
                    vec![
                        SubCommand::new(
                            "modules",
                            "List all modules",
                            vec![CommandArg::optional("filter", "Filter modules")],
                            |c, a| Commander::list_modules(c, a),
                        )
                        .alias("mods")
                        .alias("md")
                        .alias("m"),
                        SubCommand::new(
                            "ranges",
                            "List all ranges",
                            vec![CommandArg::optional("filter", "Filter ranges")],
                            |c, a| Commander::list_ranges(c, a),
                        )
                        .alias("ranges")
                        .alias("rngs")
                        .alias("rng")
                        .alias("r"),
                        SubCommand::new(
                            "functions",
                            "List all functions",
                            vec![CommandArg::optional("filter", "Filter functions")],
                            |c, a| Commander::list_functions(c, a),
                        )
                        .alias("funcs")
                        .alias("fns")
                        .alias("fn")
                        .alias("f"),
                        SubCommand::new(
                            "variables",
                            "List all variables",
                            vec![CommandArg::optional("filter", "Filter variables")],
                            |c, a| Commander::list_variables(c, a),
                        )
                        .alias("vars")
                        .alias("vrs")
                        .alias("vr")
                        .alias("v"),
                    ],
                    None,
                ),
                Command::new(
                    "read",
                    "Read data from memory",
                    vec!["r"],
                    vec![
                        CommandArg::required("address", "Address to read from"),
                        CommandArg::optional("type", "Type of data to read"),
                        CommandArg::optional("length", "Length of data to read"),
                    ],
                    vec![],
                    Some(|c, a| Commander::read(c, a)),
                ),
            ],
        }
    }

    pub fn execute_command(&mut self, command: &str, args: &[&str]) -> bool {
        if let Some(cmd) = self
            .commands
            .iter()
            .find(|c| c.command == command || c.aliases.contains(&command.to_string()))
        {
            if !cmd.subcommands.is_empty() {
                if let Some((subcommand, sub_args)) = args.split_first() {
                    if let Some(sub_cmd) = cmd.subcommands.iter().find(|s| {
                        s.name == *subcommand || s.aliases.contains(&subcommand.to_string())
                    }) {
                        // Check required arguments for the subcommand
                        let required_args = sub_cmd.args.iter().filter(|a| a.required).count();
                        if sub_args.len() < required_args {
                            println!(
                                "{} Expected at least {} arguments, got {}",
                                "Error:".red(),
                                required_args,
                                sub_args.len()
                            );
                            return true;
                        }
                        return (sub_cmd.execute)(self, sub_args);
                    }
                }
                // If we reached here, no valid subcommand was found
                if let Some(default_exec) = &cmd.default_execute {
                    return default_exec(self, args);
                }
                println!(
                    "{} {}",
                    "No subcommand specified.".red(),
                    format!("Use 'help {}' for more information.", command).dark_grey()
                );
                return true;
            } else if let Some(exec) = &cmd.default_execute {
                return exec(self, args);
            }
        } else {
            println!("{} {}", "Unknown command:".red(), command);
        }
        true
    }

    fn help(&mut self, args: &[&str]) -> bool {
        if !args.is_empty() {
            let command = self
                .commands
                .iter()
                .find(|c| c.command == args[0] || c.aliases.contains(&args[0].to_string()));
            if let Some(cmd) = command {
                // Usage
                let args_usage = cmd
                    .args
                    .iter()
                    .map(|arg| arg.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                println!(
                    "\n{} {}{}",
                    "Usage:".green(),
                    cmd.command.clone().yellow(),
                    if args_usage.is_empty() {
                        "".to_string()
                    } else {
                        format!(" {}", args_usage)
                    }
                );
                // Description
                println!("{} {}", "Description:".green(), cmd.description);
                // Arguments
                if !cmd.args.is_empty() {
                    println!("\n{}", "Arguments:".green());
                    for arg in &cmd.args {
                        let required = if arg.required { " (required)" } else { "" };
                        println!(
                            "  {:<15} {}{}",
                            format!("{}:", arg.name),
                            arg.description,
                            required.yellow()
                        );
                    }
                }

                // Aliases
                if !cmd.aliases.is_empty() {
                    println!(
                        "\n{} {}",
                        "Aliases:".green(),
                        cmd.aliases.join(", ").dark_grey()
                    );
                }

                // Subcommands
                if !cmd.subcommands.is_empty() {
                    println!("\n{}", "Subcommands:".green());
                    for sub in &cmd.subcommands {
                        let aliases = if !sub.aliases.is_empty() {
                            format!(" ({})", sub.aliases.join(", ").dark_grey())
                        } else {
                            String::new()
                        };
                        let sub_and_args = format!(
                            "{} {}",
                            sub.name,
                            sub.args
                                .iter()
                                .map(|arg| arg.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        );
                        if sub_and_args.len() > 15 {
                            println!("  {}", sub_and_args);
                            println!("  {} {}{}", " ".repeat(15), sub.description, aliases);
                        } else {
                            println!("  {:<15} {}{}", sub_and_args, sub.description, aliases);
                        }
                    }
                }

                return true;
            }

            println!("{} {}", "Unknown command:".red(), args[0]);
            true
        } else {
            // Show all commands
            println!(
                "  {}{} {}",
                "Command".green().bold(),
                " ".repeat(24 - "Command".len()),
                "Description".green().bold()
            );
            println!("  {:-<24} {:-<40}", "", "");

            for cmd in &self.commands {
                let aliases = if !cmd.aliases.is_empty() {
                    format!(" ({})", cmd.aliases.join(", ").dark_grey())
                } else {
                    String::new()
                };

                let args_usage = cmd
                    .args
                    .iter()
                    .map(|arg| arg.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");

                let cmd_with_args: String = if args_usage.is_empty() {
                    cmd.command.clone().yellow().to_string()
                } else {
                    format!("{} {}", cmd.command.clone().yellow(), args_usage)
                };
                let mut cmd_len = cmd.command.len() + args_usage.len();
                if !args_usage.is_empty() {
                    cmd_len += 1;
                }

                if cmd_len < 24 {
                    println!(
                        "  {}{} {}{}",
                        cmd_with_args,
                        " ".repeat(24 - cmd_len),
                        cmd.description,
                        aliases
                    );
                } else {
                    println!(
                        "  {}\n  {}{}",
                        cmd_with_args,
                        " ".repeat(24),
                        format!("{}{}", cmd.description, aliases)
                    );
                }

                if !cmd.subcommands.is_empty() {
                    for subcmd in &cmd.subcommands {
                        let aliases = if !subcmd.aliases.is_empty() {
                            format!(" ({})", subcmd.aliases.join(", ").dark_grey())
                        } else {
                            String::new()
                        };
                        let args_usage = subcmd
                            .args
                            .iter()
                            .map(|arg| arg.to_string())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let subcmd_with_args = if args_usage.is_empty() {
                            subcmd.name.clone().yellow().to_string()
                        } else {
                            format!("{} {}", subcmd.name.clone().yellow(), args_usage)
                        };
                        let mut subcmd_len = subcmd.name.len() + args_usage.len();
                        if !args_usage.is_empty() {
                            subcmd_len += 1;
                        }
                        if (subcmd_len + cmd.command.len() + 1) < 24 {
                            println!(
                                "  {}{}{} {}{}",
                                " ".repeat(cmd.command.len() + 1),
                                subcmd_with_args,
                                " ".repeat(24 - subcmd_len - cmd.command.len() - 1),
                                subcmd.description,
                                aliases
                            );
                        } else {
                            println!(
                                "  {}{}\n    {}{}{}",
                                " ".repeat(cmd.command.len() + 1),
                                subcmd_with_args,
                                " ".repeat(24 - 1),
                                subcmd.description,
                                aliases
                            );
                        }
                    }
                }
            }

            println!(
                "\nType \'{} {}\' for more information",
                "help".yellow().bold(),
                "[command]".bold()
            );

            true
        }
    }

    fn exit(&mut self, _args: &[&str]) -> bool {
        println!("{}", "Exiting...".yellow());
        false
    }

    fn selector(&mut self, s: &str) -> Result<Vec<&VzData>, String> {
        let re = Regex::new(r"^(?:(\w+):)?(.+)$").expect("Regex compilation failed");
        if let Some(caps) = re.captures(s) {
            let explicit_store_capture = caps.get(1);
            let selector_str = caps
                .get(2)
                .ok_or_else(|| "No selector provided".to_string())?
                .as_str();
            let selector_is_numeric = selector_str.chars().all(char::is_numeric);

            if let Some(store_match) = explicit_store_capture {
                // Store was EXPLICITLY specified
                let store_name = store_match.as_str();
                if store_name == "lib" || store_name == "l" {
                    // Explicit "lib:selector"
                    self.lib.get_data_by_selection(selector_str)
                        .map_err(|e| format!("Selector '{}': search in explicitly specified 'lib' store failed: {}", selector_str, e))
                        .and_then(|data| if data.is_empty() { Err(format!("Selector '{}': no items found in explicitly specified 'lib' store.", selector_str)) } else { Ok(data) })
                } else if store_name == "field" || store_name == "fld" || store_name == "f" {
                    // Explicit "field:selector"
                    self.field.get_data_by_selection(selector_str)
                        .map_err(|e| format!("Selector '{}': search in explicitly specified 'field' store failed: {}", selector_str, e))
                        .and_then(|data| if data.is_empty() { Err(format!("Selector '{}': no items found in explicitly specified 'field' store.", selector_str)) } else { Ok(data) })
                } else {
                    Err(format!(
                        "Unknown explicitly specified store: {}",
                        store_name
                    ))
                }
            } else {
                // NO store specified, default to "lib" with potential fallback for NUMERIC selectors
                match self.lib.get_data_by_selection(selector_str) {
                    Ok(lib_data) => {
                        if lib_data.is_empty() {
                            // Default "lib" search was empty
                            if selector_is_numeric {
                                // Selector is numeric, fallback to "field"
                                self.field.get_data_by_selection(selector_str).map_err(|field_e| {
                                    format!("Selector '{}': no items from 'lib' (default), and 'field' (fallback) search failed: {}", selector_str, field_e)
                                })
                            } else {
                                // Selector non-numeric, no fallback
                                Err(format!("Selector '{}': no items found in 'lib' (default). Non-numeric selectors do not fall back.", selector_str))
                            }
                        } else {
                            // Default "lib" search successful
                            Ok(lib_data)
                        }
                    }
                    Err(lib_e) => {
                        // Error from default "lib" store
                        if selector_is_numeric {
                            // Selector is numeric, fallback to "field"
                            self.field.get_data_by_selection(selector_str).map_err(|field_e| {
                                format!("Selector '{}': 'lib' (default) search failed (Error: {}), and 'field' (fallback) search also failed (Error: {})", selector_str, lib_e, field_e)
                            })
                        } else {
                            // Selector non-numeric, no fallback
                            Err(format!("Selector '{}': 'lib' (default) search failed (Error: {}). Non-numeric selectors do not fall back.", selector_str, lib_e))
                        }
                    }
                }
            }
        } else {
            Err(format!("Invalid selection format: {}", s))
        }
    }

    fn select(&mut self, args: &[&str]) -> bool {
        let selector = args.get(0).unwrap_or(&"");
        let result = self.selector(selector).map_err(|e| {
            println!("Failed to select data: {}", e);
            e
        });
        match result {
            Ok(data) => {
                if data.len() == 1 {
                    let item_to_select = data[0].clone();
                    self.navigator.select(&item_to_select);
                    true
                } else {
                    println!("Multiple data found for selector: {}", selector);
                    true
                }
            }
            Err(_) => true,
        }
    }

    fn deselect(&mut self, _args: &[&str]) -> bool {
        self.navigator.deselect();
        true
    }

    fn parse_number(s: &str) -> Result<u64, String> {
        if s.starts_with("0x") || s.starts_with("0X") {
            u64::from_str_radix(&s[2..], 16).map_err(|_| format!("Invalid hex number: {}", s))
        } else {
            s.parse::<u64>()
                .map_err(|_| format!("Invalid number: {}", s))
        }
    }

    fn add(&mut self, args: &[&str]) -> bool {
        match args.get(0).map(|s| Self::parse_number(s)) {
            Some(Ok(offset)) => self.navigator.add(offset),
            Some(Err(e)) => log_error(&format!("Invalid offset: {}", e)),
            None => log_error("Offset argument required"),
        }
        true
    }

    fn sub(&mut self, args: &[&str]) -> bool {
        match args.get(0).map(|s| Self::parse_number(s)) {
            Some(Ok(offset)) => self.navigator.sub(offset),
            Some(Err(e)) => log_error(&format!("Invalid offset: {}", e)),
            None => log_error("Offset argument required"),
        }
        true
    }

    fn goto(&mut self, args: &[&str]) -> bool {
        match args.get(0).map(|s| Self::parse_number(s)) {
            Some(Ok(addr)) => self.navigator.goto(addr),
            Some(Err(e)) => log_error(&format!("Invalid address: {}", e)),
            None => log_error("Address argument required"),
        }
        true
    }

    fn field_list(&mut self, args: &[&str]) -> bool {
        match args.get(0) {
            Some(v) => match v.parse::<usize>() {
                Ok(p) if p > 0 => println!("{}", self.field.to_string(Some(p - 1))),
                Ok(_) => println!("{}", self.field.to_string(None)),
                Err(_) => log_error(&format!("Invalid page number: {}", v)),
            },
            None => println!("{}", self.field.to_string(None)),
        }
        true
    }

    fn field_next(&mut self, args: &[&str]) -> bool {
        let (current_page, total_pages) = self.field.get_page_info();
        if current_page != total_pages {
            match args.get(0) {
                Some(v) => match v.parse::<u32>() {
                    Ok(p) => self.field.next_page(p.try_into().unwrap_or(1)),
                    Err(_) => log_error(&format!("Invalid page number: {}", v)),
                },
                None => self.field.next_page(1),
            }
        }
        println!("{}", self.field.to_string(None));
        true
    }

    fn field_prev(&mut self, args: &[&str]) -> bool {
        let (current_page, _) = self.field.get_page_info();
        if current_page != 1 {
            match args.get(0) {
                Some(v) => match v.parse::<u32>() {
                    Ok(p) => self.field.prev_page(p.try_into().unwrap_or(1)),
                    Err(_) => log_error(&format!("Invalid page number: {}", v)),
                },
                None => self.field.prev_page(1),
            }
        }
        println!("{}", self.field.to_string(None));
        true
    }

    fn field_sort(&mut self, args: &[&str]) -> bool {
        if let Some(sort_by) = args.get(0) {
            self.field.sort(Some(sort_by));
        }
        println!("{}", self.field.to_string(None));
        true
    }

    fn field_move(&mut self, args: &[&str]) -> bool {
        let from_res = args
            .get(0)
            .ok_or("Missing from index")
            .and_then(|v| v.parse::<usize>().map_err(|_| "Invalid from index"));
        let to_res = args
            .get(1)
            .ok_or("Missing to index")
            .and_then(|v| v.parse::<usize>().map_err(|_| "Invalid to index"));
        match (from_res, to_res) {
            (Ok(from), Ok(to)) => self.field.move_data(from, to),
            (Err(e), _) | (_, Err(e)) => log_error(&format!("Field move error: {}", e)),
        }
        println!("{}", self.field.to_string(None));
        true
    }

    fn field_remove(&mut self, args: &[&str]) -> bool {
        let index_res = args
            .get(0)
            .ok_or("Missing index")
            .and_then(|v| v.parse::<usize>().map_err(|_| "Invalid index"));
        let count_res = args
            .get(1)
            .unwrap_or(&"1")
            .parse::<usize>()
            .map_err(|_| "Invalid count");
        match (index_res, count_res) {
            (Ok(idx), Ok(count)) => self.field.remove_data(idx, count),
            (Err(e), _) | (_, Err(e)) => log_error(&format!("Field remove error: {}", e)),
        }
        println!("{}", self.field.to_string(None));
        true
    }

    fn field_clear(&mut self, _args: &[&str]) -> bool {
        self.field.clear_data();
        println!("{}", self.field.to_string(None));
        true
    }

    fn field_filter(&mut self, args: &[&str]) -> bool {
        let filter_arg = args.get(0).map_or("", |v| v);
        match parse_filter_string(filter_arg) {
            Ok(filter) => {
                self.field.filter(filter);
                println!("{}", self.field.to_string(None));
            }
            Err(e) => log_error(&format!("Failed to parse filter string: {}", e)),
        }
        true
    }

    fn lib_list(&mut self, args: &[&str]) -> bool {
        match args.get(0) {
            Some(v) => match v.parse::<usize>() {
                Ok(p) if p > 0 => println!("{}", self.lib.to_string(Some(p - 1))),
                Ok(_) => println!("{}", self.lib.to_string(None)),
                Err(_) => log_error(&format!("Invalid page number: {}", v)),
            },
            None => println!("{}", self.lib.to_string(None)),
        }
        true
    }

    fn lib_next(&mut self, args: &[&str]) -> bool {
        let (current_page, total_pages) = self.lib.get_page_info();
        if current_page != total_pages {
            match args.get(0) {
                Some(v) => match v.parse::<u32>() {
                    Ok(p) => self.lib.next_page(p.try_into().unwrap_or(1)),
                    Err(_) => log_error(&format!("Invalid page number: {}", v)),
                },
                None => self.lib.next_page(1),
            }
        }
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_prev(&mut self, args: &[&str]) -> bool {
        let (current_page, _) = self.lib.get_page_info();
        if current_page != 1 {
            match args.get(0) {
                Some(v) => match v.parse::<u32>() {
                    Ok(p) => self.lib.prev_page(p.try_into().unwrap_or(1)),
                    Err(_) => log_error(&format!("Invalid page number: {}", v)),
                },
                None => self.lib.prev_page(1),
            }
        }
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_sort(&mut self, args: &[&str]) -> bool {
        if let Some(sort_by) = args.get(0) {
            self.lib.sort(Some(sort_by));
        }
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_save(&mut self, args: &[&str]) -> bool {
        let selected = if let Some(sel) = args.get(0) {
            match self.field.get_data_by_selection(sel) {
                Ok(data) if !data.is_empty() => Ok(data),
                Ok(_) => {
                    log_error(&format!("No data found for selector: {}", sel));
                    return true;
                }
                Err(e) => {
                    log_error(&format!("Selector error: {}", e));
                    return true;
                }
            }
        } else if let Some(nav_data) = self.navigator.get_data() {
            Ok(vec![nav_data])
        } else {
            log_error("No selector provided and navigator is empty");
            return true;
        };

        if let Ok(datas) = selected {
            self.lib.add_datas(
                datas
                    .into_iter()
                    .map(|d| {
                        let mut d = d.clone();
                        match &mut d {
                            VzData::Pointer(p) => p.base.is_saved = true,
                            VzData::Module(m) => m.base.is_saved = true,
                            VzData::Range(r) => r.base.is_saved = true,
                            VzData::Function(f) => f.base.is_saved = true,
                            VzData::Variable(v) => v.base.is_saved = true,
                            VzData::JavaClass(c) => c.base.is_saved = true,
                            VzData::JavaMethod(m) => m.base.is_saved = true,
                            VzData::ObjCClass(c) => c.base.is_saved = true,
                            VzData::ObjCMethod(m) => m.base.is_saved = true,
                            VzData::Thread(t) => t.base.is_saved = true,
                        }
                        d
                    })
                    .collect(),
            );
        }
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_move(&mut self, args: &[&str]) -> bool {
        let from_res = args
            .get(0)
            .ok_or("Missing from index")
            .and_then(|v| v.parse::<usize>().map_err(|_| "Invalid from index"));
        let to_res = args
            .get(1)
            .ok_or("Missing to index")
            .and_then(|v| v.parse::<usize>().map_err(|_| "Invalid to index"));
        match (from_res, to_res) {
            (Ok(from), Ok(to)) => self.lib.move_data(from, to),
            (Err(e), _) | (_, Err(e)) => log_error(&format!("Lib move error: {}", e)),
        }
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_remove(&mut self, args: &[&str]) -> bool {
        let index_res = args
            .get(0)
            .ok_or("Missing index")
            .and_then(|v| v.parse::<usize>().map_err(|_| "Invalid index"));
        let count_res = args
            .get(1)
            .unwrap_or(&"1")
            .parse::<usize>()
            .map_err(|_| "Invalid count");
        match (index_res, count_res) {
            (Ok(idx), Ok(count)) => self.lib.remove_data(idx, count),
            (Err(e), _) | (_, Err(e)) => log_error(&format!("Lib remove error: {}", e)),
        }
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_clear(&mut self, _args: &[&str]) -> bool {
        self.lib.clear_data();
        println!("{}", self.lib.to_string(None));
        true
    }

    fn lib_filter(&mut self, args: &[&str]) -> bool {
        let filter_arg = args.get(0).map_or("", |v| v);
        match parse_filter_string(filter_arg) {
            Ok(filter) => {
                self.lib.filter(filter);
                println!("{}", self.lib.to_string(None));
            }
            Err(e) => log_error(&format!("Failed to parse filter string: {}", e)),
        }
        true
    }

    fn list_modules(&mut self, _args: &[&str]) -> bool {
        let filter = _args.get(0).map(|s| s.to_string());
        let modules = match list_modules(&mut self.script, filter.as_deref()) {
            Ok(m) => m.into_iter().map(VzData::Module).collect::<Vec<_>>(),
            Err(e) => {
                log_error(&format!("List modules error: {}", e));
                return true;
            }
        };
        self.field.clear_data();
        self.field.add_datas(modules);
        println!("{}", self.field.to_string(None));
        true
    }

    fn list_ranges(&mut self, _args: &[&str]) -> bool {
        let protect = _args.get(0).map(|s| s.to_string());
        let filter = _args.get(1).map(|s| s.to_string());
        let ranges = match list_ranges(&mut self.script, protect.as_deref(), filter.as_deref()) {
            Ok(r) => r.into_iter().map(VzData::Range).collect::<Vec<_>>(),
            Err(e) => {
                log_error(&format!("List ranges error: {}", e));
                return true;
            }
        };
        self.field.clear_data();
        self.field.add_datas(ranges);
        println!("{}", self.field.to_string(None));
        true
    }

    fn list_functions(&mut self, _args: &[&str]) -> bool {
        let filter;
        let arg0 = _args.get(0).map(|s| s.to_string()).unwrap_or_default();
        let res = self.selector(arg0.as_str());
        let module = match res {
            Ok(data) => {
                if data.is_empty() {
                    log_error("No data selected");
                    return true;
                } else if let Some(VzData::Module(m)) = data.first() {
                    filter = _args.get(1).map(|s| s.to_string());
                    m.clone()
                } else {
                    log_error("Selected data is not a module");
                    return true;
                }
            }
            Err(e) => match self.navigator.get_data() {
                Some(vz_data_from_navigator) => {
                    if let VzData::Module(m) = vz_data_from_navigator {
                        filter = _args.get(0).map(|s| s.to_string());
                        m.clone()
                    } else {
                        log_error(&format!(
                            "Selector error: {}. Navigator data is not a VzModule.",
                            e
                        ));
                        return true;
                    }
                }
                None => {
                    log_error(&format!("Selector error: {}. Navigator has no data.", e));
                    return true;
                }
            },
        };
        let functions = match list_functions(&mut self.script, module, filter.as_deref()) {
            Ok(fns) => fns.into_iter().map(VzData::Function).collect::<Vec<_>>(),
            Err(e) => {
                log_error(&format!("List functions error: {}", e));
                return true;
            }
        };
        self.field.clear_data();
        self.field.add_datas(functions);
        println!("{}", self.field.to_string(None));
        true
    }

    fn list_variables(&mut self, _args: &[&str]) -> bool {
        let filter;
        let arg0 = _args.get(0).map(|s| s.to_string()).unwrap_or_default();
        let res = self.selector(arg0.as_str());
        let module = match res {
            Ok(data) => {
                if data.is_empty() {
                    log_error("No data selected");
                    return true;
                } else if let Some(VzData::Module(m)) = data.first() {
                    filter = _args.get(1).map(|s| s.to_string());
                    m.clone()
                } else {
                    log_error("Selected data is not a module");
                    return true;
                }
            }
            Err(e) => match self.navigator.get_data() {
                Some(vz_data_from_navigator) => {
                    if let VzData::Module(m) = vz_data_from_navigator {
                        filter = _args.get(0).map(|s| s.to_string());
                        m.clone()
                    } else {
                        log_error(&format!(
                            "Selector error: {}. Navigator data is not a VzModule.",
                            e
                        ));
                        return true;
                    }
                }
                None => {
                    log_error(&format!("Selector error: {}. Navigator has no data.", e));
                    return true;
                }
            },
        };
        let variables = match list_variables(&mut self.script, module, filter.as_deref()) {
            Ok(vars) => vars.into_iter().map(VzData::Variable).collect::<Vec<_>>(),
            Err(e) => {
                log_error(&format!("List variables error: {}", e));
                return true;
            }
        };
        self.field.clear_data();
        self.field.add_datas(variables);
        println!("{}", self.field.to_string(None));
        true
    }

    fn read(&mut self, args: &[&str]) -> bool {
        // println!("{}", value);
        true
    }

    fn debug_exports(&mut self, _args: &[&str]) -> bool {
        match self.script.list_exports() {
            Ok(exports) => println!("{:?}", &exports),
            Err(e) => log_error(&format!("Failed to list exports: {}", e)),
        }
        true
    }
}
