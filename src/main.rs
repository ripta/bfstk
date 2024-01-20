use snafu::prelude::*;
use std::io::Read;
use std::result;
use std::time::Instant;

fn main() {
    let flags = parse_args();
    if let Err(e) = flags {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = run(flags.unwrap()) {
        eprintln!("Error: {}", e);
        std::process::exit(2);
    }
}

type Result<T> = result::Result<T, BFE>;

#[derive(Debug)]
struct Flags {
    files: Vec<String>,
    with_report: bool,
}

fn parse_args() -> Result<Flags> {
    let mut args = pico_args::Arguments::from_env();
    let mut flags = Flags {
        files: Vec::new(),
        with_report: args.contains("-r"),
    };

    let rem = args.finish();
    if !rem.is_empty() {
        flags.files = rem.iter().map(|s| s.to_string_lossy().to_string()).collect();
    }

    return Ok(flags);
}

fn run(flags: Flags) -> Result<u8> {
    for filename in flags.files {
        let mut ts = vec![("start", Instant::now())];

        let content = std::fs::read_to_string(filename.clone()).context(FileLoadSnafu {
            filename: filename.clone(),
        })?;
        ts.push(("read", Instant::now()));

        let tokens = lex(content)?;
        ts.push(("lex", Instant::now()));

        let nodes = parse(tokens)?;
        ts.push(("parse", Instant::now()));

        let mut state = State::new();
        for node in nodes {
            state = eval(state, node)?;
        }
        ts.push(("eval", Instant::now()));

        if flags.with_report {
            eprintln!("State:");
            eprintln!("  counter: {}", state.counter);
            eprintln!("  memory: {} {}", state.data_left.len(), state.data_right.len());

            eprintln!("Timings:");
            for t in ts.windows(2) {
                eprintln!("  {}: {:.2?}", &t[1].0, &t[1].1.duration_since(t[0].1));
            }
        }
    }

    return Ok(0);
}

#[derive(Debug, Snafu)]
enum BFE {
    #[snafu(display("cannot load file '{filename}'"))]
    FileLoad {
        source: std::io::Error,
        filename: String,
    },
    #[snafu(display("stack underflow: {reason}"))]
    StackUnderflow {
        reason: String,
    },
    UnclosedJump,
    #[snafu(display("x"))]
    ReadInput {
        source: std::io::Error,
    },
    #[snafu(display("BUG! internal invariant violated: {reason}"))]
    InvariantViolation {
        reason: String,
    },
    Unknown,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
}

#[derive(Debug, Clone)]
enum TokenKind {
    Comment(char),

    DecrementByte, // -
    IncrementByte, // +

    MoveRight, // >
    MoveLeft,  // <

    Input,  // ,
    Output, // .

    JumpRight, // [ // also jump-if-zero
    JumpLeft,  // ] // also jump-if-nonzero
}

/// lex scans through the input and coverts each character into a token. No
/// transformation happens at this step.
fn lex(content: String) -> Result<Vec<Token>> {
    let mut toks = Vec::with_capacity(content.len());
    for ch in content.chars() {
        let kind = match ch {
            '-' => TokenKind::DecrementByte,
            '+' => TokenKind::IncrementByte,
            '>' => TokenKind::MoveRight,
            '<' => TokenKind::MoveLeft,
            ',' => TokenKind::Input,
            '.' => TokenKind::Output,
            '[' => TokenKind::JumpRight,
            ']' => TokenKind::JumpLeft,
            c => TokenKind::Comment(c),
        };

        toks.push(Token { kind });
    }
    return Ok(toks);
}

/// Node represents a node that could have been combined from one or more tokens.
#[derive(Debug, Clone)]
enum Node {
    // Comment is a comment string, which in brainfuck could be anything that
    // isn't an instruction.
    Comment(String),
    // Delta represents a series of one or more increments and/or decrements
    // in a row. By convention, net positive increments results in a positive
    // delta value, and net positive decrements results in a negative delta value.
    // Net zeros are not yet elided.
    Delta(i8),
    // Move represents a series of one or more cell moves left or right. By
    // convention, moves right have positive values, while moves left have
    // negative values.
    Move(i16),
    // Read is an instruction to read one u8 character from STDIN.
    Read,
    // Write is an instruction to write one u8 character to STDOUT.
    Write,
    // Block is a list of parsed nodes from between a JumpRight and JumpLeft
    // pair of tokens.
    Block(Vec<Node>),
}

/// parse runs through the list of tokens, coalescing similar tokens in a row
/// if they are safe to combine, and emits a list of parsed nodes.
fn parse(tokens: Vec<Token>) -> Result<Vec<Node>> {
    let mut spans: Vec<Vec<Node>> = vec![vec![]];
    let mut span = spans.last_mut().context(InvariantViolationSnafu {
        reason: "expecting 'spans' stack to not be empty",
    })?;

    for token in tokens {
        match token.kind {
            // a comment can be combined into the same comment node, when the
            // previous token was also a comment
            TokenKind::Comment(b) => match span.last_mut() {
                Some(Node::Comment(a)) => a.push(b),
                _ => span.push(Node::Comment(b.to_string())),
            },

            // a decrement or an increment can be combined when the previous
            // node was a delta, which can only happen if the token was also
            // either a decrement or an increment
            TokenKind::DecrementByte => match span.last_mut() {
                Some(Node::Delta(a)) => {
                    *a -= 1;
                }
                _ => {
                    span.push(Node::Delta(-1));
                }
            },
            TokenKind::IncrementByte => match span.last_mut() {
                Some(Node::Delta(a)) => {
                    *a += 1;
                }
                _ => {
                    span.push(Node::Delta(1));
                }
            },

            // moves right or left can be combined when the previous node
            // was a move, which only happen if the previous token was also
            // either a move right or left
            TokenKind::MoveRight => match span.last_mut() {
                Some(Node::Move(a)) => {
                    *a += 1;
                }
                _ => {
                    span.push(Node::Move(1));
                }
            },
            TokenKind::MoveLeft => match span.last_mut() {
                Some(Node::Move(a)) => {
                    *a -= 1;
                }
                _ => {
                    span.push(Node::Move(-1));
                }
            },

            TokenKind::Input => span.push(Node::Read),
            TokenKind::Output => span.push(Node::Write),

            TokenKind::JumpRight => {
                spans.push(vec![]);
                span = spans.last_mut().context(InvariantViolationSnafu {
                    reason: "expecting 'spans' stack to not be empty when encountering JumpRight token",
                })?;
            }
            TokenKind::JumpLeft => match spans.pop() {
                None => {
                    return Err(BFE::StackUnderflow {
                        reason: "expecting 'spans' stack to not be empty when encountering JumpLeft token (None case)"
                            .to_string(),
                    })
                }
                Some(prev) => {
                    span = spans.last_mut().context(StackUnderflowSnafu {
                        reason: "found closing jump-if-nonzero ']' without a corresponding opening jump-if-zero '['",
                    })?;
                    span.push(Node::Block(prev));
                }
            },
        }
    }

    if spans.len() > 1 {
        return Err(BFE::StackUnderflow {
            reason: "found jump-if-zero '[' that was not closed with a jump-if-nonzero ']'".to_string(),
        });
    }

    let f = spans.first().context(InvariantViolationSnafu {
        reason: "expecting 'spans' stack to not be empty at end of parsing",
    })?;
    return Ok(f.clone());
}

#[derive(Debug, Clone)]
struct State {
    counter: usize,
    pointer: i16,
    data_right: Vec<u8>,
    data_left: Vec<u8>,
}

impl State {
    fn new() -> State {
        return State {
            counter: 0,
            pointer: 0,
            data_right: vec![0],
            data_left: vec![],
        };
    }
}

fn eval(mut state: State, node: Node) -> Result<State> {
    match node {
        Node::Comment(_) => {}

        Node::Delta(i) if state.pointer < 0 => {
            state.counter += 1;
            if i < 0 {
                state.data_left[-state.pointer as usize] -= (-i) as u8;
            } else {
                state.data_left[-state.pointer as usize] += i as u8;
            }
        }
        Node::Delta(i) => {
            state.counter += 1;
            if i < 0 {
                state.data_right[state.pointer as usize] -= (-i) as u8;
            } else {
                state.data_right[state.pointer as usize] += i as u8;
            }
        }

        Node::Move(i) => {
            state.counter += 1;
            state.pointer += i;
            if state.pointer < 0 {
                while (-state.pointer) as usize >= state.data_left.len() {
                    state.data_left.push(0u8);
                }
            } else {
                while state.pointer as usize >= state.data_right.len() {
                    state.data_right.push(0u8);
                }
            }
        }

        Node::Read => {
            state.counter += 1;

            let mut c = [0u8; 1];
            std::io::stdin().read_exact(&mut c).context(ReadInputSnafu)?;
            if state.pointer < 0 {
                state.data_left[-state.pointer as usize] = c[0];
            } else {
                state.data_right[state.pointer as usize] = c[0];
            }
        }
        Node::Write if state.pointer < 0 => {
            state.counter += 1;
            print!("{}", state.data_left[-state.pointer as usize] as char);
        }
        Node::Write => {
            state.counter += 1;
            print!("{}", state.data_right[state.pointer as usize] as char);
        }

        Node::Block(subprogram) => {
            state.counter += 1;
            while state.data_right[state.pointer as usize] != 0 {
                for node in subprogram.clone() {
                    state = eval(state, node)?;
                }
            }
        }
    }

    return Ok(state);
}
