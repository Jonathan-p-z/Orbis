use crate::ast::*;
use anyhow::bail;

fn is_op(tok: &str) -> bool {
    matches!(tok, "|" | ">" | ">>" | "<" | "2>" | "&")
}

/// shell-words handles quoting, but operators glued to words (ls|wc) need a second pass.
fn tokenize(line: &str) -> anyhow::Result<Vec<String>> {
    let base = shell_words::split(line)?;
    let mut out = Vec::new();
    for t in base {
        let mut cur = String::new();
        for ch in t.chars() {
            if matches!(ch, '|' | '<' | '>' | '&') {
                if !cur.is_empty() {
                    out.push(cur.clone());
                    cur.clear();
                }
                out.push(ch.to_string());
            } else {
                cur.push(ch);
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }

    // glue >> and 2> back together — the char loop above split them
    let mut i = 0;
    let mut fixed = Vec::new();
    while i < out.len() {
        if out[i] == ">" && i + 1 < out.len() && out[i + 1] == ">" {
            fixed.push(">>".to_string());
            i += 2;
            continue;
        }
        if out[i] == "2" && i + 1 < out.len() && out[i + 1] == ">" {
            fixed.push("2>".to_string());
            i += 2;
            continue;
        }
        fixed.push(out[i].clone());
        i += 1;
    }

    Ok(fixed)
}

pub fn parse_line(line: &str) -> anyhow::Result<Option<Pipeline>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }

    let toks = tokenize(line)?;
    if toks.is_empty() {
        return Ok(None);
    }

    let mut pipeline = Pipeline { cmds: Vec::new(), background: false };
    let mut cur = Command { argv: Vec::new(), redirects: Vec::new() };

    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();

        if t == "&" {
            if i != toks.len() - 1 {
                bail!("`&` must be at end of line");
            }
            pipeline.background = true;
            i += 1;
            continue;
        }

        if t == "|" {
            if cur.argv.is_empty() {
                bail!("invalid pipeline: empty command before |");
            }
            pipeline.cmds.push(cur);
            cur = Command { argv: Vec::new(), redirects: Vec::new() };
            i += 1;
            continue;
        }

        if matches!(t, ">" | ">>" | "<" | "2>") {
            let op = t;
            let Some(target) = toks.get(i + 1) else { bail!("redirect with no target"); };
            let (fd, mode) = match op {
                "<" => (Fd::Stdin, RedirectMode::Read),
                ">" => (Fd::Stdout, RedirectMode::WriteTrunc),
                ">>" => (Fd::Stdout, RedirectMode::WriteAppend),
                "2>" => (Fd::Stderr, RedirectMode::WriteTrunc),
                _ => unreachable!(),
            };
            cur.redirects.push(Redirect {
                fd,
                mode,
                target: RedirectTarget::Path(target.clone()),
            });
            i += 2;
            continue;
        }

        if is_op(t) {
            bail!("unexpected operator: {t}");
        }

        cur.argv.push(toks[i].clone());
        i += 1;
    }

    if cur.argv.is_empty() {
        bail!("empty command");
    }
    pipeline.cmds.push(cur);
    Ok(Some(pipeline))
}
