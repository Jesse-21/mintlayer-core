// Copyright (c) 2023 RBB S.r.l
// opensource@mintlayer.org
// SPDX-License-Identifier: MIT
// Licensed under the MIT License;
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://github.com/mintlayer/mintlayer-core/blob/master/LICENSE
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod wallet_completions;
mod wallet_prompt;

use clap::{Command, FromArgMatches, Subcommand};
use node_comm::node_traits::NodeInterface;
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, DefaultValidator, EditCommand, EditMode, Emacs, ExampleHighlighter,
    FileBackedHistory, KeyCode, KeyModifiers, Keybindings, ListMenu, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, Vi,
};

use crate::{
    cli_println,
    commands::{handle_wallet_command, WalletCommands},
    config::WalletCliConfig,
    errors::WalletCliError,
    output::OutputContext,
    repl::{wallet_completions::WalletCompletions, wallet_prompt::WalletPrompt},
    DefaultWallet,
};

const HISTORY_FILE_NAME: &str = "history.txt";
const HISTORY_MAX_LINES: usize = 1000;

fn get_repl_command() -> Command {
    // Strip out usage
    const PARSER_TEMPLATE: &str = "\
        {all-args}
    ";

    // Strip out name/version
    const APPLET_TEMPLATE: &str = "\
        {about-with-newline}\n\
        {usage-heading}\n    {usage}\n\
        \n\
        {all-args}{after-help}\
    ";

    let repl_command = Command::new("repl")
        .multicall(true)
        .arg_required_else_help(true)
        .subcommand_required(true)
        .subcommand_value_name("APPLET")
        .subcommand_help_heading("APPLETS")
        .help_template(PARSER_TEMPLATE);

    let mut repl_command = WalletCommands::augment_subcommands(repl_command);

    for subcommand in repl_command.get_subcommands_mut() {
        *subcommand = subcommand.clone().help_template(APPLET_TEMPLATE);
    }

    repl_command
}

fn parse_input(line: &str, repl_command: &Command) -> Result<WalletCommands, WalletCliError> {
    let args = shlex::split(line).ok_or(WalletCliError::InvalidQuoting)?;
    let mut matches = repl_command
        .clone()
        .try_get_matches_from(args)
        .map_err(WalletCliError::InvalidCommandInput)?;
    let command = WalletCommands::from_arg_matches_mut(&mut matches)
        .map_err(WalletCliError::InvalidCommandInput)?;
    Ok(command)
}

pub async fn start_cli_repl(
    output: &OutputContext,
    config: &WalletCliConfig,
    mut rpc_client: impl NodeInterface,
    mut wallet: DefaultWallet,
) -> Result<(), WalletCliError> {
    let repl_command = get_repl_command();

    cli_println!(output, "Use 'help' to see all available commands.");
    cli_println!(output, "Use 'exit' or Ctrl-D to quit.");

    let history_file_path = config.data_dir.join(HISTORY_FILE_NAME);
    let history = Box::new(
        FileBackedHistory::with_file(HISTORY_MAX_LINES, history_file_path.clone())
            .map_err(|e| WalletCliError::HistoryFileError(history_file_path, e))?,
    );

    let commands = repl_command
        .get_subcommands()
        .map(|command| command.get_name().to_owned())
        .chain(std::iter::once("help".to_owned()))
        .collect::<Vec<_>>();

    let completer = Box::new(WalletCompletions::new(commands.clone()));

    let mut line_editor = Reedline::create()
        .with_history(history)
        .with_completer(completer)
        .with_quick_completions(false)
        .with_partial_completions(true)
        .with_highlighter(Box::new(ExampleHighlighter::new(commands)))
        .with_validator(Box::new(DefaultValidator))
        .with_ansi_colors(true);

    // Adding default menus for the compiled reedline
    line_editor = line_editor
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            ColumnarMenu::default().with_name("completion_menu"),
        )))
        .with_menu(ReedlineMenu::HistoryMenu(Box::new(
            ListMenu::default().with_name("history_menu"),
        )));

    let edit_mode: Box<dyn EditMode> = if config.vi_mode {
        let mut normal_keybindings = default_vi_normal_keybindings();
        let mut insert_keybindings = default_vi_insert_keybindings();

        add_menu_keybindings(&mut normal_keybindings);
        add_menu_keybindings(&mut insert_keybindings);

        Box::new(Vi::new(insert_keybindings, normal_keybindings))
    } else {
        let mut keybindings = default_emacs_keybindings();
        add_menu_keybindings(&mut keybindings);

        Box::new(Emacs::new(keybindings))
    };

    line_editor = line_editor.with_edit_mode(edit_mode);

    let prompt = WalletPrompt::new();

    loop {
        let sig = line_editor.read_line(&prompt);

        match sig {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if !line.is_empty() {
                    let res = parse_input(line, &repl_command);
                    match res {
                        Ok(command) => {
                            let res = handle_wallet_command(
                                output,
                                &mut rpc_client,
                                &mut wallet,
                                &mut line_editor,
                                command,
                            )
                            .await;

                            match res {
                                Ok(_) => {}
                                Err(WalletCliError::Exit) => break Ok(()),
                                Err(e) => {
                                    cli_println!(output, "{}", e);
                                }
                            }
                        }
                        Err(e) => {
                            cli_println!(output, "{}", e);
                        }
                    }
                }
            }
            Ok(Signal::CtrlC) => {
                // Prompt has been cleared and should start on the next line
            }
            Ok(Signal::CtrlD) => {
                break Ok(());
            }
            Err(err) => {
                cli_println!(output, "Error: {err:?}");
            }
        }
    }
}

fn add_menu_keybindings(keybindings: &mut Keybindings) {
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('x'),
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("history_menu".to_string()),
            ReedlineEvent::MenuPageNext,
        ]),
    );

    keybindings.add_binding(
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyCode::Char('x'),
        ReedlineEvent::MenuPagePrevious,
    );

    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::Edit(vec![EditCommand::Complete]),
        ]),
    );

    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
}
