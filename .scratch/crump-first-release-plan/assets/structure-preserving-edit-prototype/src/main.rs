mod model;

use std::io::{self, Write};

use model::{Action, Editor};

fn main() {
    let mut editor = Editor::new();
    loop {
        print!("\x1b[2J\x1b[H{}\n> ", editor.render());
        io::stdout().flush().expect("flush stdout");

        let mut input = String::new();
        if io::stdin().read_line(&mut input).expect("read command") == 0 {
            break;
        }
        let parts: Vec<_> = input.split_whitespace().collect();
        let action = match parts.as_slice() {
            ["gain", value] => Some(Action::SetGain((*value).into())),
            ["point", name, value] => Some(Action::SetPoint {
                name: (*name).into(),
                value: (*value).into(),
            }),
            ["rename", name] => Some(Action::RenameTrack((*name).into())),
            ["cutoff", value] => Some(Action::SetCutoff((*value).into())),
            ["conflict"] => Some(Action::SimulateConflict),
            ["undo"] => Some(Action::Undo),
            ["redo"] => Some(Action::Redo),
            ["reload"] => Some(Action::Reload),
            ["quit" | "q"] => break,
            _ => None,
        };
        if let Some(action) = action {
            editor.dispatch(action);
        }
    }
}
