//! docstring 的清理与 Args 提取，不求值函数。
//! 参考：docs/README.md（Interface）。

use crate::ast::{FunctionDef, StmtKind};
use std::collections::BTreeMap;

pub fn extract(f: &FunctionDef) -> (String, BTreeMap<String, String>) {
    let text = match f.body.first().map(|s| &s.kind) {
        Some(StmtKind::Expr(e)) => e.string().unwrap_or(""),
        _ => "",
    };
    split(&clean(text))
}

// Python 的空白还包含 U+001C..U+001F，Rust 的 White_Space 不含它们。
fn whitespace(c: char) -> bool {
    c.is_whitespace() || (0x1c..=0x1f).contains(&(c as u32))
}

fn clean(text: &str) -> String {
    let mut expanded = String::new();
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let n = 8 - column % 8;
            expanded.extend(std::iter::repeat_n(' ', n));
            column += n;
        } else {
            expanded.push(c);
            column = if c == '\n' || c == '\r' {
                0
            } else {
                column + 1
            };
        }
    }
    let mut lines: Vec<String> = expanded.split('\n').map(str::to_owned).collect();
    let margin = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim_matches(whitespace).is_empty())
        .map(|l| l.chars().take_while(|c| whitespace(*c)).count())
        .min()
        .unwrap_or(0);
    lines[0] = lines[0].trim_start_matches(whitespace).to_owned();
    for line in lines.iter_mut().skip(1) {
        *line = line.chars().skip(margin).collect();
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    lines.join("\n")
}

fn split(doc: &str) -> (String, BTreeMap<String, String>) {
    let lines: Vec<_> = doc.split('\n').collect();
    let Some(i) = lines
        .iter()
        .position(|l| l.trim_matches(whitespace) == "Args:")
    else {
        return (doc.trim_matches(whitespace).into(), BTreeMap::new());
    };
    let mut args = BTreeMap::new();
    let mut current: Option<(String, usize)> = None;
    for line in &lines[i + 1..] {
        if line.trim_matches(whitespace).is_empty() {
            continue;
        }
        let depth = line.chars().take_while(|c| whitespace(*c)).count();
        if current.as_ref().is_none_or(|(_, indent)| depth <= *indent) {
            let (name, text) = line
                .trim_matches(whitespace)
                .split_once(':')
                .unwrap_or((line.trim_matches(whitespace), ""));
            args.insert(
                name.trim_matches(whitespace).into(),
                text.trim_matches(whitespace).into(),
            );
            current = Some((name.trim_matches(whitespace).into(), depth));
        } else {
            let value = args.get_mut(&current.as_ref().unwrap().0).unwrap();
            *value = format!("{value} {}", line.trim_matches(whitespace))
                .trim_matches(whitespace)
                .into();
        }
    }
    (lines[..i].join("\n").trim_matches(whitespace).into(), args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleandoc_and_args_follow_the_reference() {
        let text = "  一句话。\n\n\t第二段。\n\n\tArgs:\n\t    z: 首行\n\t       续行\n\t    a: 第二个\n\t";
        assert_eq!(
            split(&clean(text)),
            (
                "一句话。\n\n第二段。".into(),
                BTreeMap::from([
                    ("z".into(), "首行 续行".into()),
                    ("a".into(), "第二个".into())
                ])
            )
        );
        assert_eq!(clean("\n    x\n      y\n"), "x\n  y");
    }
}
