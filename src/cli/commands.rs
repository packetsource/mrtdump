//! Command grammar for the SSH shell: a static tree of keyword / argument
//! nodes that drives parsing, TAB completion and `?` context help.
//!
//! Parsing is purely synchronous and has no knowledge of the routing table
//! or the SSH session; the result is a [`Command`] value that the shell
//! either acts on itself (`output`, `terminal length`, `exit`) or hands to
//! `handlers::dispatch`.

use std::net::IpAddr;
use std::str::FromStr;

use regex::Regex;

use crate::community::Community;
use crate::prefix::Prefix;

// ─── Command values ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Cisco,
    Juniper,
    Csv,
}

impl OutputFormat {
    /// Map the command line `-j` / `-t` switches onto the initial format.
    pub fn from_flags(juniper: bool, terse: bool) -> Self {
        if juniper {
            OutputFormat::Juniper
        } else if terse {
            OutputFormat::Csv
        } else {
            OutputFormat::Cisco
        }
    }
}

#[derive(Debug)]
pub enum Command {
    ShowRouteAddr(IpAddr),
    ShowRoutePrefix(Prefix),
    ShowRouteCommunity(Community),
    ShowRouteCommunityRegex(Regex),
    ShowRouteAsPathRegex((Regex, bool)),
    Output(OutputFormat),
    TerminalLength(usize),
    Exit,
}

// ─── Grammar tree ─────────────────────────────────────────────────────────────

enum Arg {
    Ip(IpAddr),
    Prefix(Prefix),
    Community(Community),
    Regex(Regex),
    AsPathRegex(Regex, bool),
    Number(usize),
}

type Action = fn(Vec<Arg>) -> Command;

#[derive(Clone, Copy)]
enum ArgKind {
    Ip,
    Prefix,
    Community,
    Regex,  // for community
    AsPathRegex,
    Number,
}

#[derive(Clone, Copy)]
enum Kind {
    Keyword(&'static str),
    Arg(ArgKind),
}

struct Node {
    kind: Kind,
    help: &'static str,
    /// Present when the line may legitimately end at this node (`<cr>`).
    action: Option<Action>,
    children: &'static [Node],
}

const fn kw(
    name: &'static str,
    help: &'static str,
    action: Option<Action>,
    children: &'static [Node],
) -> Node {
    Node { kind: Kind::Keyword(name), help, action, children }
}

const fn arg(
    kind: ArgKind,
    help: &'static str,
    action: Option<Action>,
    children: &'static [Node],
) -> Node {
    Node { kind: Kind::Arg(kind), help, action, children }
}

const MAX_TERMINAL_LENGTH: usize = 512;

static TREE: &[Node] = &[
    kw("show", "Show routing table information", None, &[
        kw("route", "Show routing table entries", None, &[
            arg(ArgKind::Ip, "Longest-prefix match for an IPv4/IPv6 address",
                Some(act_route_addr), &[]),
            arg(ArgKind::Prefix, "Exact prefix and length (IPv4/IPv6)",
                Some(act_route_prefix), &[]),
            kw("community", "Routes carrying a BGP community", None, &[
                arg(ArgKind::Community, "Standard (A:B) or large (A:B:C) community",
                    Some(act_community), &[]),
                arg(ArgKind::Regex, "Regular expression over community strings",
                    Some(act_community_regex), &[]),
            ]),
            kw("as-path", "Routes whose AS path matches a regular expression", None, &[
                arg(ArgKind::AsPathRegex, "Quoted regular expression, e.g. \"^64496 .* 64511$\"",
                    Some(act_aspath_regex), &[]),
            ]),
        ]),
    ]),
    kw("output", "Select the route output format", None, &[
        kw("cisco", "Cisco IOS 'show ip bgp' style", Some(act_out_cisco), &[]),
        kw("juniper", "Juniper 'show route' style", Some(act_out_juniper), &[]),
        kw("csv", "Terse comma-separated values", Some(act_out_csv), &[]),
    ]),
    kw("terminal", "Set terminal parameters", None, &[
        kw("length", "Lines per page before --More-- (0 disables paging)", None, &[
            arg(ArgKind::Number, "Number of lines (0-512)", Some(act_term_length), &[]),
        ]),
    ]),
    kw("exit", "Close the session", Some(act_exit), &[]),
    kw("quit", "Close the session", Some(act_exit), &[]),
];

fn act_route_addr(mut a: Vec<Arg>) -> Command {
    let Some(Arg::Ip(ip)) = a.pop() else { unreachable!() };
    Command::ShowRouteAddr(ip)
}
fn act_route_prefix(mut a: Vec<Arg>) -> Command {
    let Some(Arg::Prefix(p)) = a.pop() else { unreachable!() };
    Command::ShowRoutePrefix(p)
}
fn act_community(mut a: Vec<Arg>) -> Command {
    let Some(Arg::Community(c)) = a.pop() else { unreachable!() };
    Command::ShowRouteCommunity(c)
}
fn act_community_regex(mut a: Vec<Arg>) -> Command {
    let Some(Arg::Regex(r)) = a.pop() else { unreachable!() };
    Command::ShowRouteCommunityRegex(r)
}
fn act_aspath_regex(mut a: Vec<Arg>) -> Command {
    let Some(Arg::AsPathRegex(re, re_modified)) = a.pop() else { unreachable!() };
    Command::ShowRouteAsPathRegex((re, re_modified))
}
fn act_term_length(mut a: Vec<Arg>) -> Command {
    let Some(Arg::Number(n)) = a.pop() else { unreachable!() };
    Command::TerminalLength(n)
}
fn act_out_cisco(_: Vec<Arg>) -> Command { Command::Output(OutputFormat::Cisco) }
fn act_out_juniper(_: Vec<Arg>) -> Command { Command::Output(OutputFormat::Juniper) }
fn act_out_csv(_: Vec<Arg>) -> Command { Command::Output(OutputFormat::Csv) }
fn act_exit(_: Vec<Arg>) -> Command { Command::Exit }

impl ArgKind {
    fn label(self) -> &'static str {
        match self {
            ArgKind::Ip => "<ip-address>",
            ArgKind::Prefix => "<prefix/length>",
            ArgKind::Community => "<A:B | A:B:C>",
            ArgKind::Regex => "<regex>",
            ArgKind::AsPathRegex => "<regex>",
            ArgKind::Number => "<0-512>",
        }
    }

    fn parse(self, tok: &Token) -> Result<Arg, String> {
        let s = tok.text.as_str();
        match self {
            ArgKind::Ip => IpAddr::from_str(s)
                .map(Arg::Ip)
                .map_err(|_| "Invalid IP address".to_string()),
            ArgKind::Prefix => {
                let p = Prefix::from_str(s).map_err(|_| "Invalid prefix".to_string())?;
                let max = if p.prefix.is_ipv4() { 32 } else { 128 };
                if p.len > max {
                    return Err(format!("Prefix length out of range (0-{max})"));
                }
                Ok(Arg::Prefix(p))
            }
            ArgKind::Community => Community::from_str(s)
                .map(Arg::Community)
                .map_err(|_| "Invalid community".to_string()),
            ArgKind::Regex => Regex::new(s).map(Arg::Regex).map_err(|e| {
                let text = e.to_string();
                let last = text.lines().last().unwrap_or("").trim_start_matches("error: ");
                format!("Invalid regular expression: {last}")
            }),
            ArgKind::AsPathRegex => {
                let fixed = s.replace('_', " ");
                let replaced = fixed != s;
                Regex::new(&fixed).map(|re| Arg::AsPathRegex(re, replaced)).map_err(|e| {
                    let text = e.to_string();
                    let last = text.lines().last().unwrap_or("").trim_start_matches("error: ");
                    format!("Invalid regular expression: {last}")
                })
                // compile(&fixed).map(|re| Arg::AsPathRegex(re, replaced))
            }
            ArgKind::Number => match s.parse::<usize>() {
                Ok(n) if n <= MAX_TERMINAL_LENGTH => Ok(Arg::Number(n)),
                _ => Err(format!("Invalid number (0-{MAX_TERMINAL_LENGTH})")),
            },
        }
    }
}

// ─── Tokenizer ────────────────────────────────────────────────────────────────

struct Token {
    text: String,
    /// Byte offset of the token's first character in the line.
    start: usize,
    quoted: bool,
    /// A quote was opened but not (yet) closed.
    open: bool,
}

struct Tokens {
    tokens: Vec<Token>,
    /// The line ends in whitespace outside a quote, i.e. a new token is due.
    trailing_space: bool,
}

/// Split on whitespace, honouring double quotes. Inside quotes only `\"` is
/// unescaped; every other backslash is kept so regexes pass through intact.
fn tokenize(line: &str) -> Tokens {
    let mut tokens = Vec::new();
    let mut cur: Option<Token> = None;
    let mut in_quote = false;
    let mut chars = line.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if in_quote {
            let t = cur.as_mut().expect("quote implies token");
            if c == '\\' && matches!(chars.peek(), Some((_, '"'))) {
                chars.next();
                t.text.push('"');
            } else if c == '"' {
                in_quote = false;
                t.open = false;
            } else {
                t.text.push(c);
            }
        } else if c.is_whitespace() {
            tokens.extend(cur.take());
        } else {
            let t = cur.get_or_insert_with(|| Token {
                text: String::new(),
                start: i,
                quoted: false,
                open: false,
            });
            if c == '"' {
                t.quoted = true;
                t.open = true;
                in_quote = true;
            } else {
                t.text.push(c);
            }
        }
    }
    tokens.extend(cur);

    Tokens {
        tokens,
        trailing_space: !in_quote && line.ends_with(char::is_whitespace),
    }
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ErrorKind {
    Invalid(Option<String>),
    Ambiguous,
    Incomplete,
}

#[derive(Debug)]
pub struct ParseError {
    /// Byte offset of the offending token within the line.
    pub pos: usize,
    pub token: String,
    pub kind: ErrorKind,
}

impl ParseError {
    /// Format for the client. With `caret`, a `^` marker is drawn under the
    /// offending token (`prompt_len` accounts for the echoed prompt); without
    /// it (exec requests, piped input) the token is quoted instead.
    pub fn render(&self, prompt_len: usize, caret: bool) -> String {
        let marker = if caret {
            format!("{}^\n", " ".repeat(prompt_len + self.pos))
        } else {
            String::new()
        };
        match &self.kind {
            ErrorKind::Incomplete => "% Incomplete command.\n".to_string(),
            ErrorKind::Ambiguous => format!("{marker}% Ambiguous command: \"{}\"\n", self.token),
            ErrorKind::Invalid(Some(msg)) => format!("{marker}% {msg}\n"),
            ErrorKind::Invalid(None) if caret => {
                format!("{marker}% Invalid input detected at '^' marker.\n")
            }
            ErrorKind::Invalid(None) => {
                format!("% Invalid input detected at '{}'.\n", self.token)
            }
        }
    }
}

// ─── Walking the tree ─────────────────────────────────────────────────────────

enum MatchErr {
    Ambiguous,
    NoMatch(Option<String>),
}

fn keyword_name(n: &Node) -> Option<&'static str> {
    match n.kind {
        Kind::Keyword(k) => Some(k),
        Kind::Arg(_) => None,
    }
}

/// Match one token against the candidate children: keywords first (exact,
/// then unique abbreviation), then typed arguments in declaration order.
fn match_child(
    children: &'static [Node],
    tok: &Token,
) -> Result<(&'static Node, Option<Arg>), MatchErr> {
    if !tok.quoted {
        let want = tok.text.to_ascii_lowercase();
        if let Some(n) = children.iter().find(|n| keyword_name(n) == Some(want.as_str())) {
            return Ok((n, None));
        }
        let abbrev: Vec<&Node> = children
            .iter()
            .filter(|n| keyword_name(n).is_some_and(|k| k.starts_with(&want)))
            .collect();
        match abbrev.len() {
            0 => {}
            1 => return Ok((abbrev[0], None)),
            _ => return Err(MatchErr::Ambiguous),
        }
    }

    let mut last_err = None;
    for n in children {
        if let Kind::Arg(kind) = n.kind {
            match kind.parse(tok) {
                Ok(a) => return Ok((n, Some(a))),
                Err(e) => last_err = Some(e),
            }
        }
    }
    Err(MatchErr::NoMatch(last_err))
}

struct Walk {
    /// Candidates for the next token.
    children: &'static [Node],
    last: Option<&'static Node>,
    args: Vec<Arg>,
}

fn walk(tokens: &[Token]) -> Result<Walk, ParseError> {
    let mut w = Walk { children: TREE, last: None, args: Vec::new() };
    for tok in tokens {
        match match_child(w.children, tok) {
            Ok((node, a)) => {
                w.children = node.children;
                w.last = Some(node);
                w.args.extend(a);
            }
            Err(MatchErr::Ambiguous) => {
                return Err(ParseError {
                    pos: tok.start,
                    token: tok.text.clone(),
                    kind: ErrorKind::Ambiguous,
                })
            }
            Err(MatchErr::NoMatch(msg)) => {
                return Err(ParseError {
                    pos: tok.start,
                    token: tok.text.clone(),
                    kind: ErrorKind::Invalid(msg),
                })
            }
        }
    }
    Ok(w)
}

/// Parse a complete command line.
pub fn parse(line: &str) -> Result<Command, ParseError> {
    let t = tokenize(line);
    if let Some(tok) = t.tokens.last().filter(|t| t.open) {
        return Err(ParseError {
            pos: tok.start,
            token: tok.text.clone(),
            kind: ErrorKind::Invalid(Some("Unterminated quoted string".to_string())),
        });
    }
    let w = walk(&t.tokens)?;
    match w.last.and_then(|n| n.action) {
        Some(action) => Ok(action(w.args)),
        None => Err(ParseError {
            pos: line.len(),
            token: String::new(),
            kind: ErrorKind::Incomplete,
        }),
    }
}

// ─── Completion and help ──────────────────────────────────────────────────────

pub struct Suggest {
    /// Byte offset where the token being completed starts.
    pub start: usize,
    /// The partial token typed so far (empty when a new token is due).
    pub partial: String,
    /// Matching keywords with their help text.
    pub keywords: Vec<(&'static str, &'static str)>,
    /// Typed-argument slots valid here, with their help text.
    pub args: Vec<(&'static str, &'static str)>,
    /// The line is already a complete command (`<cr>` is valid).
    pub cr: bool,
}

/// What may follow `prefix` (the text left of the cursor).
pub fn suggest(prefix: &str) -> Result<Suggest, ParseError> {
    let t = tokenize(prefix);
    let (done, partial) = match t.tokens.split_last() {
        Some((last, rest)) if !t.trailing_space => (rest, Some(last)),
        _ => (&t.tokens[..], None),
    };
    let w = walk(done)?;

    let (start, text, quoted) = match partial {
        Some(p) => (p.start, p.text.clone(), p.quoted),
        None => (prefix.len(), String::new(), false),
    };
    let want = text.to_ascii_lowercase();

    let keywords: Vec<_> = if quoted {
        Vec::new()
    } else {
        w.children
            .iter()
            .filter_map(|n| keyword_name(n).map(|k| (k, n.help)))
            .filter(|(k, _)| k.starts_with(&want))
            .collect()
    };
    let args = if partial.is_none() || keywords.is_empty() {
        w.children
            .iter()
            .filter_map(|n| match n.kind {
                Kind::Arg(a) => Some((a.label(), n.help)),
                Kind::Keyword(_) => None,
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(Suggest {
        start,
        partial: text,
        keywords,
        args,
        cr: partial.is_none() && w.last.is_some_and(|n| n.action.is_some()),
    })
}

impl Suggest {
    pub fn render_help(&self) -> String {
        let mut s = String::new();
        for (name, help) in self.keywords.iter().chain(self.args.iter()) {
            s.push_str(&format!("  {name:<18}{help}\n"));
        }
        if self.cr {
            s.push_str("  <cr>\n");
        }
        if s.is_empty() {
            s.push_str("% Unrecognized command\n");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(line: &str) -> Command {
        parse(line).unwrap_or_else(|e| panic!("{line:?}: {e:?}"))
    }

    #[test]
    fn show_route_forms() {
        assert!(matches!(ok("show route 192.0.2.1"), Command::ShowRouteAddr(_)));
        assert!(matches!(ok("show route 2001:db8::1"), Command::ShowRouteAddr(_)));
        assert!(matches!(ok("show route 192.0.2.0/24"), Command::ShowRoutePrefix(_)));
        assert!(matches!(ok("show route 2001:db8::/32"), Command::ShowRoutePrefix(_)));
        assert!(matches!(
            ok("show route community 65000:100"),
            Command::ShowRouteCommunity(Community::Standard((65000, 100)))
        ));
        assert!(matches!(
            ok("show route community 4200000000:1:2"),
            Command::ShowRouteCommunity(Community::Large(_))
        ));
        assert!(matches!(ok("show route community ^65.*:100$"), Command::ShowRouteCommunityRegex(_)));
        assert!(matches!(ok("sh ro as \"^64496 .* 64511$\""), Command::ShowRouteAsPathRegex(_)));
    }

    #[test]
    fn quoted_regex_keeps_spaces_and_backslashes() {
        let Command::ShowRouteAsPathRegex((re, re_modified)) = ok(r#"show route as-path "a \d+ \"b\"""#) else {
            panic!()
        };
        assert_eq!(re.as_str(), r#"a \d+ "b""#);
    }

    #[test]
    fn errors() {
        assert!(matches!(parse("show route").unwrap_err().kind, ErrorKind::Incomplete));
        assert!(matches!(parse("show route 1.2.3.4/33").unwrap_err().kind, ErrorKind::Invalid(Some(_))));
        assert!(matches!(parse("show route as-path \"(\"").unwrap_err().kind, ErrorKind::Invalid(Some(_))));
        assert!(matches!(parse("show route as-path \"abc").unwrap_err().kind, ErrorKind::Invalid(Some(_))));
        assert_eq!(parse("show bogus").unwrap_err().pos, 5);
        assert!(matches!(parse("e").unwrap().into_exit(), true));
    }

    #[test]
    fn terminal_and_output() {
        assert!(matches!(ok("terminal length 0"), Command::TerminalLength(0)));
        assert!(parse("terminal length 9999").is_err());
        assert!(matches!(ok("output juniper"), Command::Output(OutputFormat::Juniper)));
    }

    #[test]
    fn suggestions() {
        let s = suggest("sh").unwrap();
        assert_eq!((s.start, s.keywords.len()), (0, 1));
        let s = suggest("show route ").unwrap();
        assert_eq!(s.keywords.len(), 2);
        assert_eq!(s.args.len(), 2);
        let s = suggest("show route community 6").unwrap();
        assert!(s.keywords.is_empty() && s.args.len() == 2);
        assert!(suggest("output csv ").unwrap().cr);
        assert!(suggest("output csv").unwrap().cr == false); // partial token, not yet a <cr>
        assert!(suggest("nonsense ").is_err());
    }

    impl Command {
        fn into_exit(self) -> bool {
            matches!(self, Command::Exit)
        }
    }
}
