use tree_sitter::{Parser, Tree};
use tree_sitter_bash::LANGUAGE as BASH;

thread_local! {
    static PARSER: std::cell::RefCell<Parser> = std::cell::RefCell::new({
        let mut p = Parser::new();
        p.set_language(&BASH.into()).expect("load bash grammar");
        p
    });
}

/// Parse the provided bash source using tree-sitter-bash, returning a Tree on
/// success or None if parsing failed.
pub fn try_parse_bash(bash_lc_arg: &str) -> Option<Tree> {
    PARSER.with(|parser| {
        parser.borrow_mut().parse(bash_lc_arg, None)
    })
}

/// Parse a script which may contain multiple simple commands joined only by
/// the safe logical/pipe/sequencing operators: `&&`, `||`, `;`, `|`.
///
/// Returns `Some(Vec<command_words>)` if every command is a plain word‑only
/// command and the parse tree does not contain disallowed constructs
/// (parentheses, redirections, substitutions, control flow, etc.). Otherwise
/// returns `None`.
pub fn try_parse_word_only_commands_sequence(tree: &Tree, src: &str) -> Option<Vec<Vec<String>>> {
    if tree.root_node().has_error() {
        return None;
    }

    let mut commands = Vec::new();
    let mut cursor = tree.walk();
    
    fn visit_node(node: tree_sitter::Node, src: &str, commands: &mut Vec<Vec<String>>, cursor: &mut tree_sitter::TreeCursor) -> bool {
        match node.kind() {
            "command" => {
                if let Some(words) = parse_plain_command_from_node(node, src) {
                    commands.push(words);
                } else {
                    return false;
                }
            }
            // Allowed containers
            "program" | "list" | "pipeline" => {
                for child in node.children(cursor) {
                    if !visit_node(child, src, commands, cursor) {
                        return false;
                    }
                }
            }
            // Allowed tokens
            "&&" | "||" | ";" | "|" | "\"" | "'" => {}
            // Whitespace
            kind if kind.trim().is_empty() => {}
            // Reject everything else
            _ => return false,
        }
        true
    }
    
    if visit_node(tree.root_node(), src, &mut commands, &mut cursor) {
        Some(commands)
    } else {
        None
    }
}

fn parse_plain_command_from_node(cmd: tree_sitter::Node, src: &str) -> Option<Vec<String>> {
    if cmd.kind() != "command" {
        return None;
    }
    
    let src_bytes = src.as_bytes();
    let mut words = Vec::new();
    let mut cursor = cmd.walk();
    
    for child in cmd.named_children(&mut cursor) {
        let text = match child.kind() {
            "command_name" => {
                let word_node = child.named_child(0)?;
                if word_node.kind() != "word" { return None; }
                word_node.utf8_text(src_bytes).ok()?
            }
            "word" | "number" => child.utf8_text(src_bytes).ok()?,
            "string" => {
                if child.child_count() == 3 {
                    child.child(1)?.utf8_text(src_bytes).ok()?
                } else {
                    return None;
                }
            }
            "raw_string" => {
                let raw = child.utf8_text(src_bytes).ok()?;
                raw.strip_prefix('\'')?.strip_suffix('\'')?  
            }
            _ => return None,
        };
        words.push(text.to_owned());
    }
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_seq(src: &str) -> Option<Vec<Vec<String>>> {
        let tree = try_parse_bash(src)?;
        try_parse_word_only_commands_sequence(&tree, src)
    }

    #[test]
    fn accepts_single_simple_command() {
        let cmds = parse_seq("ls -1").unwrap();
        assert_eq!(cmds, vec![vec!["ls".to_string(), "-1".to_string()]]);
    }

    #[test]
    fn accepts_multiple_commands_with_allowed_operators() {
        let src = "ls && pwd; echo 'hi there' | wc -l";
        let cmds = parse_seq(src).unwrap();
        let expected: Vec<Vec<String>> = vec![
            vec!["ls".to_string()],
            vec!["pwd".to_string()],
            vec!["echo".to_string(), "hi there".to_string()],
            vec!["wc".to_string(), "-l".to_string()],
        ];
        assert_eq!(cmds, expected);
    }

    #[test]
    fn extracts_double_and_single_quoted_strings() {
        let cmds = parse_seq("echo \"hello world\"").unwrap();
        assert_eq!(
            cmds,
            vec![vec!["echo".to_string(), "hello world".to_string()]]
        );

        let cmds2 = parse_seq("echo 'hi there'").unwrap();
        assert_eq!(
            cmds2,
            vec![vec!["echo".to_string(), "hi there".to_string()]]
        );
    }

    #[test]
    fn accepts_numbers_as_words() {
        let cmds = parse_seq("echo 123 456").unwrap();
        assert_eq!(
            cmds,
            vec![vec![
                "echo".to_string(),
                "123".to_string(),
                "456".to_string()
            ]]
        );
    }

    #[test]
    fn rejects_parentheses_and_subshells() {
        assert!(parse_seq("(ls)").is_none());
        assert!(parse_seq("ls || (pwd && echo hi)").is_none());
    }

    #[test]
    fn rejects_redirections_and_unsupported_operators() {
        assert!(parse_seq("ls > out.txt").is_none());
        assert!(parse_seq("echo hi & echo bye").is_none());
    }

    #[test]
    fn rejects_command_and_process_substitutions_and_expansions() {
        assert!(parse_seq("echo $(pwd)").is_none());
        assert!(parse_seq("echo `pwd`").is_none());
        assert!(parse_seq("echo $HOME").is_none());
        assert!(parse_seq("echo \"hi $USER\"").is_none());
    }

    #[test]
    fn rejects_variable_assignment_prefix() {
        assert!(parse_seq("FOO=bar ls").is_none());
    }

    #[test]
    fn rejects_trailing_operator_parse_error() {
        assert!(parse_seq("ls &&").is_none());
    }
}
