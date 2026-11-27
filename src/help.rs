//! Local onboarding text; no configuration inspection or network access.
use std::io::IsTerminal;

pub fn color_enabled(terminal: bool, no_color: bool, dumb_terminal: bool) -> bool {
    terminal && !no_color && !dumb_terminal
}

pub fn print(missing_application: bool) {
    let terminal = if missing_application {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    let color = color_enabled(
        terminal,
        std::env::var_os("NO_COLOR").is_some(),
        std::env::var_os("TERM").is_some_and(|term| term == "dumb"),
    );
    let text = render(missing_application, color);
    if missing_application {
        eprint!("{text}");
    } else {
        print!("{text}");
    }
}

fn render(missing_application: bool, color: bool) -> String {
    let (heading, accent, warning, reset) = if color {
        ("\x1b[1;36m", "\x1b[36m", "\x1b[33m", "\x1b[0m")
    } else {
        ("", "", "", "")
    };
    let mut text = format!(
        "\n{heading}Bootleg Smuggler{reset}\n\
         Run your app. Let Smuggler handle container registration.\n\n"
    );
    if missing_application {
        text.push_str(&format!(
            "{warning}Let's give Smuggler an application to run.{reset}\n\
             Put your command after --; Smuggler is an entrypoint, not a standalone server.\n\n"
        ));
    }
    text.push_str(&format!(
        "{heading}Try it locally — no account or network needed{reset}\n\
         \x20 BOOTLEG_DISABLED=true cargo run -- -- /bin/echo 'Hello from Smuggler!'\n\n\
         \x20 The first -- belongs to Cargo; the second belongs to Smuggler.\n\
         \x20 With the built binary: smuggler -- /bin/echo 'Hello from Smuggler!'\n\n\
         {heading}Run your application{reset}\n\
         \x20 {accent}smuggler [--name LABEL] [--name-env VARIABLE] -- COMMAND [ARGS...]{reset}\n\n\
         \x20 --name LABEL         Give this container a display name.\n\
         \x20 --name-env VARIABLE  Read its name from an environment variable.\n\
         \x20 --help               Show this guide.\n\n\
         {heading}Ready to register a container?{reset}\n\
         \x20 Set BOOTLEG_PROJECT_ID to your public project ID. Inject\n\
         \x20 BOOTLEG_PROJECT_TOKEN at runtime from your secret manager.\n\
         \x20 Never put the token in an image, build argument, or source control.\n\
         \x20 Set BOOTLEG_API_URL to your Bootleg server's HTTPS origin.\n\
         \x20 It is required for registration and has no default (see README.md).\n\n\
         \x20 With both project ID and token absent, Smuggler simply runs your app.\n\
         \x20 BOOTLEG_DISABLED=true always skips registration and networking.\n\
         \x20 When enabled, control-plane outages won't stop your app.\n\n\
         More examples: README.md and examples/kind.yaml\n\n"
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_is_only_for_capable_interactive_terminals() {
        assert!(color_enabled(true, false, false));
        assert!(!color_enabled(false, false, false));
        assert!(!color_enabled(true, true, false));
        assert!(!color_enabled(true, false, true));
    }

    #[test]
    fn color_does_not_change_guidance() {
        let colored = render(true, true);
        let plain = render(true, false);
        assert!(colored.contains("\x1b[1;36m"));
        let stripped = ["\x1b[1;36m", "\x1b[36m", "\x1b[33m", "\x1b[0m"]
            .iter()
            .fold(colored, |text, escape| text.replace(escape, ""));
        assert_eq!(stripped, plain);
        assert!(!plain.contains('\x1b'));
        assert!(plain.contains("cargo run -- --"));
        assert!(!render(false, false).contains("Let's give"));
    }
}
