/*
Copyright © 2023 Fabio Lenherr

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <http://www.gnu.org/licenses/>.
*/

use serde_derive::Deserialize;
use std::{
    env, fs, io::Read, os::unix::net::UnixStream, path::PathBuf, process::exit, process::Command,
    thread, time::Duration,
};
use toml;

#[derive(Deserialize)]
struct HyprDock {
    monitor_name: String,
    open_bar_command: String,
    close_bar_command: String,
    reload_bar_command: String,
    suspend_command: String,
    lock_command: String,
    utility_command: String,
    get_monitors_command: String,
    enable_internal_monitor_command: String,
    disable_internal_monitor_command: String,
    enable_external_monitor_command: String,
    disable_external_monitor_command: String,
    extend_command: String,
    mirror_command: String,
    wallpaper_command: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }

    let dock = parse_config(
        home::home_dir()
            .unwrap()
            .join(PathBuf::from(".config/hypr/hyprdock.toml"))
            .to_str()
            .unwrap(),
    );

    let mut iter = args.iter();
    iter.next();
    let mut iteration = 0;
    for _ in 0..args.len() - 1 {
        if iteration == args.len() - 1 {
            break;
        }
        iteration += 1;
        match iter.next().unwrap().as_str() {
            "--internal" | "-i" => dock.internal_monitor(),
            "--external" | "-e" => dock.external_monitor(),
            "--extend" | "-eo" => dock.extend_monitor(),
            "--mirror" | "-io" => dock.mirror_monitor(),
            "--server" | "-s" => dock.socket_connect(),
            "--suspend" | "-su" => dock.lock_system(),
            "--version" | "-v" => println!("0.2.1"),
            "--help" | "-h" => {
                print_help();
                return;
            }
            x => {
                println!("Could not parse {}", x);
                print_help();
                return;
            }
        }
    }
}

fn print_help() {
    print!(
        "Possible arguments are:
            --extend/-e:    Extends monitors
            --mirror/-m:    Mirrors monitors
            --internal/-io: Switch to internal monitor only
            --external/-eo: Switch to external monitor only
            --server/-s:    daemon version
                            automatically handles actions on laptop lid close and open.
            --bar/-b:       selects a bar to start when monitor switches (used for eww)
            --help/-h:      shows options
            --version/-v:   shows version\n"
    );
}

fn parse_config(path: &str) -> HyprDock {
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => String::from(
            r#"monitor_name = 'eDP-1'
            open_bar_command = 'eww open bar'
            close_bar_command = 'eww close-all'
            reload_bar_command = 'eww reload'
            suspend_command = 'systemctl suspend'
            lock_command = 'swaylock -c 000000'
            utility_command = 'playerctl --all-players -a pause'
            get_monitors_command = 'hyprctl monitors'
            enable_internal_monitor_command = 'hyprctl keyword monitor {monitor_name},highrr,0x0,1'
            disable_internal_monitor_command = 'hyprctl keyword monitor {monitor_name},diabled'
            enable_external_monitor_command = 'hyprctl keyword monitor ,highrr,0x0,1'
            disable_external_monitor_command = 'hyprctl keyword monitor ,disabled'
            extend_command = 'hyprctl keyword monitor ,highrr,1920x0,1'
            mirror_command = 'hyprctl keyword monitor ,highrr,0x0,1'
            wallpaper_command = 'hyprctl dispatch hyprpaper'"#,
        ),
    };
    match toml::from_str(&contents) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("Unable to load data from `{}`", path);
            exit(1);
        }
    }
}

impl HyprDock {
    pub fn execute_command(&self, command: &str) {
        let command_split: Vec<&str> = command.split(" ").collect();
        if command_split.len() == 0 {
            return;
        }
        let (first, rest) = command_split.split_first().unwrap();
        Command::new(first)
            .args(rest)
            .spawn()
            .expect("Could not parse command, please check your toml");
    }

    pub fn execute_command_with_output(&self, command: &str) -> Vec<u8> {
        let command_split: Vec<&str> = command.split(" ").collect();
        if command_split.len() == 0 {
            return Vec::new();
        }
        let (first, rest) = command_split.split_first().unwrap();
        Command::new(first)
            .args(rest)
            .output()
            .expect("Could not parse command, please check your toml")
            .stdout
    }

    pub fn handle_close(&self) {
        if self.has_external_monitor() {
            self.external_monitor();
            thread::sleep(Duration::from_millis(1000));
            self.restart_hyprpaper();
            self.restart_eww_bar();
        } else {
            self.stop_music();
            self.lock_system();
        }
    }

    pub fn handle_open(&self) {
        if self.is_internal_active() {
            return;
        }
        if !self.has_external_monitor() {
            self.internal_monitor();
            self.restart_hyprpaper();
            self.restart_eww_bar();
            self.fix_eww_bar();
            return;
        } else {
            self.internal_monitor();
            self.extend_monitor();
            self.restart_hyprpaper();
            self.restart_eww_bar();
            self.fix_eww_bar();
        }
    }

    pub fn handle_event(&self, event: &str) {
        match event {
            "button/lid LID close\n" => self.handle_close(),
            "button/lid LID open\n" => self.handle_open(),
            _ => {}
        }
    }

    pub fn socket_connect(&self) {
        let mut sock =
            UnixStream::connect("/var/run/acpid.socket").expect("failed to connect to socket");
        loop {
            let mut buf = [0; 1024];
            let n = sock.read(&mut buf).expect("failed to read from socket");
            let data = std::str::from_utf8(&buf[..n]).unwrap().to_string();

            self.handle_event(data.as_str());
        }
    }

    pub fn lock_system(&self) {
        self.execute_command(self.lock_command.as_str());
        self.execute_command(self.suspend_command.as_str());
    }

    pub fn stop_music(&self) {
        self.execute_command(self.utility_command.as_str());
    }

    pub fn extend_monitor(&self) {
        if !self.is_internal_active() {
            self.restart_internal();
        }
        self.execute_command(self.extend_command.as_str());
    }

    pub fn mirror_monitor(&self) {
        if !self.is_internal_active() {
            self.restart_internal();
        }
        self.execute_command(self.mirror_command.as_str());
    }

    pub fn internal_monitor(&self) {
        let needs_restart = !self.is_internal_active();
        self.execute_command(self.enable_internal_monitor_command.as_str());
        self.execute_command(self.disable_external_monitor_command.as_str());
        if needs_restart {
            self.restart_eww_bar();
            self.restart_hyprpaper();
        }
    }

    pub fn restart_internal(&self) {
        self.execute_command(self.enable_internal_monitor_command.as_str());
        self.restart_hyprpaper();
        self.restart_eww_bar();
        self.fix_eww_bar();
    }

    pub fn external_monitor(&self) {
        if !self.has_external_monitor() {
            return;
        }
        let needs_restart = !self.is_internal_active();
        self.execute_command(self.disable_internal_monitor_command.as_str());
        self.execute_command(self.enable_external_monitor_command.as_str());
        if needs_restart {
            self.restart_eww_bar();
            self.restart_hyprpaper();
        }
    }

    pub fn restart_hyprpaper(&self) {
        self.execute_command(self.wallpaper_command.as_str());
    }

    pub fn restart_eww_bar(&self) {
        self.execute_command(self.close_bar_command.as_str());
        self.execute_command(self.open_bar_command.as_str());
    }

    pub fn fix_eww_bar(&self) {
        self.execute_command(self.reload_bar_command.as_str());
    }

    pub fn is_internal_active(&self) -> bool {
        let output =
            String::from_utf8(self.execute_command_with_output(self.get_monitors_command.as_str()))
                .unwrap();
        if output.contains(self.monitor_name.as_str()) {
            return true;
        }
        false
    }

    pub fn has_external_monitor(&self) -> bool {
        let output =
            String::from_utf8(self.execute_command_with_output(self.get_monitors_command.as_str()))
                .unwrap();
        if output.contains("ID 1") {
            return true;
        }
        false
    }
}
