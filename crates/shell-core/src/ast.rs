#[derive(Debug, Clone)]
pub enum RedirectTarget {
    Path(String),
}

#[derive(Debug, Clone, Copy)]
pub enum RedirectMode {
    Read,         // <
    WriteTrunc,   // >
    WriteAppend,  // >>
}

#[derive(Debug, Clone, Copy)]
pub enum Fd {
    Stdout,
    Stderr,
    Stdin,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub fd: Fd,
    pub mode: RedirectMode,
    pub target: RedirectTarget,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub argv: Vec<String>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub cmds: Vec<Command>,
    pub background: bool,
}
