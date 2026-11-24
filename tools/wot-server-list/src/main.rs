#![forbid(unsafe_code)]

use std::{
    fs::{self, OpenOptions},
    net::SocketAddrV4,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand};
use tempfile::NamedTempFile;
use wot_platform::client_formats::pxml::{self, Element, Value};

#[derive(Parser)]
#[command(version, about = "Manage the World of Tanks EU server list")]
struct Command {
    #[command(subcommand)]
    operation: Operation,
}

#[derive(Subcommand)]
enum Operation {
    /// Insert a server entry with an unused periphery ID.
    Insert(Insert),
    /// List the currently configured servers.
    List {
        /// World of Tanks installation directory.
        #[arg(long, value_name = "DIRECTORY")]
        game_dir: PathBuf,
    },
    /// Delete an entry by periphery ID and leave its public key file in place.
    Delete {
        /// World of Tanks installation directory.
        #[arg(long, value_name = "DIRECTORY")]
        game_dir: PathBuf,
        /// Periphery ID of the entry to delete.
        #[arg(long, value_parser = clap::value_parser!(u32).range(0..=i64::from(i32::MAX)))]
        periphery_id: u32,
    },
}

#[derive(Args)]
struct Insert {
    /// World of Tanks installation directory.
    #[arg(long, value_name = "DIRECTORY")]
    game_dir: PathBuf,
    /// Login server IPv4 address and UDP port.
    #[arg(long, value_name = "IP:PORT", default_value = "127.0.0.1:20016")]
    address: SocketAddrV4,
    /// Server name shown in the game.
    #[arg(long, default_value = "WoT Local")]
    name: String,
    /// Short server name, unique in the client's list.
    #[arg(long, default_value = "WOTLOCAL")]
    short_name: String,
    /// Server-list ID, matching the server's --periphery-id.
    #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(0..=i64::from(i32::MAX)))]
    periphery_id: u32,
    /// Public key filename referenced in the game's res folder [default: loginapp_wot_server_<PERIPHERY_ID>.pubkey].
    #[arg(long)]
    key_name: Option<String>,
}

fn main() -> Result<()> {
    match Command::parse().operation {
        Operation::Insert(command) => {
            let key_name = command.key_name()?;
            edit_hosts(&command.game_dir, |login| {
                add_host(login, &command, &key_name)
            })?;
            println!("Inserted {} ({})", command.name, command.address);
        }
        Operation::List { game_dir } => list_hosts(&game_dir)?,
        Operation::Delete {
            game_dir,
            periphery_id,
        } => {
            edit_hosts(&game_dir, |login| delete_host(login, periphery_id))?;
            println!("Deleted server with periphery ID {periphery_id}");
        }
    }
    Ok(())
}

impl Insert {
    fn key_name(&self) -> Result<String> {
        ensure!(self.address.port() != 0, "Server port must be nonzero");
        ensure!(!self.name.trim().is_empty(), "Server name must be nonempty");
        ensure!(
            !self.short_name.trim().is_empty(),
            "Short name must be nonempty"
        );
        let key_name = self
            .key_name
            .clone()
            .unwrap_or_else(|| format!("loginapp_wot_server_{}.pubkey", self.periphery_id));
        ensure!(
            !key_name.is_empty()
                && !key_name.starts_with('.')
                && !key_name.ends_with('.')
                && key_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
            "--key-name requires a filename in the game's res folder using letters, digits, dots, underscores or hyphens"
        );

        Ok(key_name)
    }
}

fn read_config(path: &Path) -> Result<Box<Element>> {
    let bytes = fs::read(path).with_context(|| format!("Read {}", path.display()))?;
    pxml::from_bytes(&bytes).with_context(|| format!("Decode Packed XML in {}", path.display()))
}

fn list_hosts(game_dir: &Path) -> Result<()> {
    let config = game_dir.join("res/scripts_config.xml");
    let root = read_config(&config)?;
    let login = root
        .get_child_element("login")
        .with_context(|| format!("{} has no login section", config.display()))?;
    println!("PERIPHERY ID\tSHORT NAME\tADDRESS\tPUBLIC KEY\tNAME");
    for value in login.iter_children("host") {
        let host = value.as_element().context("Invalid login/host entry")?;
        let id = host
            .get_child_integer("periphery_id")
            .map_or_else(|| "-".into(), |id| id.to_string());
        println!(
            "{id}\t{}\t{}\t{}\t{}",
            host.get_child_string("short_name").unwrap_or("-"),
            host.get_child_string("url").unwrap_or("-"),
            host.get_child_string("public_key_path").unwrap_or("-"),
            host.get_child_string("name").unwrap_or("-")
        );
    }
    Ok(())
}

fn edit_hosts(game_dir: &Path, edit: impl FnOnce(&mut Element) -> Result<()>) -> Result<()> {
    let res = game_dir.join("res");
    let config = res.join("scripts_config.xml");
    let permissions = fs::metadata(&config)
        .with_context(|| format!("Find {}", config.display()))?
        .permissions();
    // The lock stays on a separate file because the config is replaced by rename.
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(res.join(".wot-server-list.lock"))
        .context("Open server-list lock")?;
    lock.lock().context("Lock the game server list")?;

    let mut root = read_config(&config)?;
    let Some(Value::Element(login)) = root.get_child_mut("login") else {
        anyhow::bail!("{} has no login section", config.display());
    };

    edit(login)?;

    let mut pending = NamedTempFile::new_in(&res).context("Stage the game server list")?;
    pxml::to_writer(&mut pending, &root).context("Encode the game server list")?;
    pending.as_file().set_permissions(permissions)?;
    pending.as_file().sync_all()?;
    pending
        .persist(&config)
        .with_context(|| format!("Replace {}", config.display()))?;
    Ok(())
}

fn add_host(login: &mut Element, command: &Insert, key_name: &str) -> Result<()> {
    let address = command.address.to_string();
    let id = i64::from(command.periphery_id);
    let fields = [
        ("name", command.name.as_str()),
        ("short_name", command.short_name.as_str()),
        ("url", address.as_str()),
        ("url_token", address.as_str()),
        ("public_key_path", key_name),
    ];

    for value in login.iter_children("host") {
        let host = value.as_element().context("Invalid login/host entry")?;
        let label = host
            .get_child_string("name")
            .unwrap_or("an existing server");
        ensure!(
            host.get_child_integer("periphery_id") != Some(id),
            "Periphery ID {id} is already used by {label}. Choose another --periphery-id and use it on the server too"
        );
        ensure!(
            !host
                .get_child_string("short_name")
                .is_some_and(|name| name.eq_ignore_ascii_case(&command.short_name)),
            "Short name {} is already used by {label}. Choose another --short-name",
            command.short_name
        );
    }

    let mut host = Element::new();
    for (name, value) in fields {
        host.add_children(name, Value::String(value.into()));
    }
    host.add_children("periphery_id", Value::Integer(id));
    login.add_children("host", Value::Element(Box::new(host)));
    Ok(())
}

fn delete_host(login: &mut Element, periphery_id: u32) -> Result<()> {
    let id = i64::from(periphery_id);
    let index = {
        let mut matches =
            login
                .iter_children_all()
                .enumerate()
                .filter_map(|(index, (name, value))| {
                    (name == "host"
                        && value.as_element()?.get_child_integer("periphery_id") == Some(id))
                    .then_some(index)
                });
        let index = matches
            .next()
            .with_context(|| format!("No server has periphery ID {id}"))?;
        ensure!(
            matches.next().is_none(),
            "Multiple servers have periphery ID {id}"
        );
        index
    };
    login
        .remove_child(index)
        .context("Server entry disappeared")?;
    Ok(())
}
