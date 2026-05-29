use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ToolCommands {
    /// List all available tools
    List,
}

pub async fn run(cmd: &ToolCommands) {
    match cmd {
        ToolCommands::List => list(),
    }
}

fn list() {
    let defs = crate::api::tool_definitions();
    if defs.is_empty() {
        println!("No tools registered.");
        return;
    }

    println!("Available tools ({}):", defs.len());
    println!();
    for d in &defs {
        let name = d["function"]["name"].as_str().unwrap_or("?");
        let desc = d["function"]["description"].as_str().unwrap_or("");
        let params = &d["function"]["parameters"];
        let props = params["properties"].as_object();
        let required = params["required"].as_array();

        println!("  \x1B[1m{name}\x1B[0m");
        if !desc.is_empty() {
            println!("    {desc}");
        }
        if let Some(props) = props {
            let param_list: Vec<String> = props.keys().map(|k| {
                let ptype = props[k]["type"].as_str().unwrap_or("?");
                let is_req = required.map(|r| r.iter().any(|v| v == k)).unwrap_or(false);
                format!("{}{}{}", k, if is_req { "*" } else { "" }, if ptype != "?" { format!(": {ptype}") } else { String::new() })
            }).collect();
            if !param_list.is_empty() {
                println!("    params: {}", param_list.join(", "));
            }
        }
        println!();
    }
}
