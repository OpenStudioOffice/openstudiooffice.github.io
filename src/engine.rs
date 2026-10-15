//! Spreadsheet engine: cell addressing, formula parser, evaluator and CSV.
//! Pure Rust, no dependencies, so it can be unit-tested on its own.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// (column, row), both 0-based. A1 = (0, 0).
pub type Pos = (usize, usize);

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
    Err(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Empty => Ok(()),
            Value::Num(n) => write!(f, "{}", format_num(*n)),
            Value::Text(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{}", if *b { "TRUE" } else { "FALSE" }),
            Value::Err(e) => write!(f, "{e}"),
        }
    }
}

fn format_num(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let s = format!("{:.10}", n);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 0 -> "A", 25 -> "Z", 26 -> "AA" ...
pub fn col_name(mut c: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s
}

/// "B3" or "$B$3" -> Some((1, 2))
pub fn parse_cell_ref(s: &str) -> Option<Pos> {
    let s = s.replace('$', "");
    let letters: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &s[letters.len()..];
    if letters.is_empty()
        || letters.len() > 3
        || digits.is_empty()
        || !digits.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let mut col = 0usize;
    for ch in letters.chars() {
        col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    let row: usize = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((col - 1, row - 1))
}

// ───────────────────────────── Sheet ─────────────────────────────

#[derive(Default, Clone)]
pub struct Sheet {
    pub cells: HashMap<Pos, String>,
}

impl Sheet {
    pub fn get(&self, pos: Pos) -> Option<&str> {
        self.cells.get(&pos).map(|s| s.as_str())
    }

    pub fn set(&mut self, pos: Pos, raw: &str) {
        if raw.is_empty() {
            self.cells.remove(&pos);
        } else {
            self.cells.insert(pos, raw.to_string());
        }
    }

    /// Evaluate every cell and return the computed values.
    pub fn evaluate_all(&self) -> HashMap<Pos, Value> {
        let mut ev = Evaluator::new(self);
        let keys: Vec<Pos> = self.cells.keys().cloned().collect();
        for k in keys {
            ev.value(k);
        }
        ev.cache
    }

    pub fn to_csv(&self) -> String {
        if self.cells.is_empty() {
            return String::new();
        }
        let (mut max_c, mut max_r) = (0usize, 0usize);
        for &(c, r) in self.cells.keys() {
            max_c = max_c.max(c);
            max_r = max_r.max(r);
        }
        let mut out = String::new();
        for r in 0..=max_r {
            let row: Vec<String> = (0..=max_c)
                .map(|c| csv_escape(self.get((c, r)).unwrap_or("")))
                .collect();
            out.push_str(&row.join(","));
            out.push('\n');
        }
        out
    }

    pub fn from_csv(text: &str, max_cols: usize, max_rows: usize) -> Sheet {
        fn store_row(sheet: &mut Sheet, r: usize, row: &[String], mc: usize, mr: usize) {
            if r >= mr {
                return;
            }
            for (c, s) in row.iter().enumerate() {
                if c < mc {
                    sheet.set((c, r), s);
                }
            }
        }

        let mut sheet = Sheet::default();
        let mut field = String::new();
        let mut row: Vec<String> = Vec::new();
        let mut in_quotes = false;
        let mut r = 0usize;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if in_quotes {
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        field.push('"');
                        chars.next();
                    } else {
                        in_quotes = false;
                    }
                } else {
                    field.push(ch);
                }
            } else {
                match ch {
                    '"' if field.is_empty() => in_quotes = true,
                    ',' => row.push(std::mem::take(&mut field)),
                    '\r' => {}
                    '\n' => {
                        row.push(std::mem::take(&mut field));
                        store_row(&mut sheet, r, &row, max_cols, max_rows);
                        row.clear();
                        r += 1;
                    }
                    _ => field.push(ch),
                }
            }
        }
        if !field.is_empty() || !row.is_empty() {
            row.push(field);
            store_row(&mut sheet, r, &row, max_cols, max_rows);
        }
        sheet
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ───────────────────────────── Tokenizer ─────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Colon,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut out = Vec::new();
    while i < n {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' => {
                let start = i;
                while i < n && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                if i < n && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < n && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < n && chars[j].is_ascii_digit() {
                        while j < n && chars[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                let v: f64 = text.parse().map_err(|_| "#VALUE!".to_string())?;
                out.push(Tok::Num(v));
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                loop {
                    if i >= n {
                        return Err("#PARSE!".to_string());
                    }
                    if chars[i] == '"' {
                        if i + 1 < n && chars[i + 1] == '"' {
                            s.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                out.push(Tok::Str(s));
            }
            'a'..='z' | 'A'..='Z' | '$' | '_' => {
                let start = i;
                while i < n
                    && (chars[i].is_ascii_alphanumeric()
                        || chars[i] == '$'
                        || chars[i] == '_'
                        || chars[i] == '.')
                {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            '<' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    out.push(Tok::Op("<="));
                    i += 2;
                } else if i + 1 < n && chars[i + 1] == '>' {
                    out.push(Tok::Op("<>"));
                    i += 2;
                } else {
                    out.push(Tok::Op("<"));
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < n && chars[i + 1] == '=' {
                    out.push(Tok::Op(">="));
                    i += 2;
                } else {
                    out.push(Tok::Op(">"));
                    i += 1;
                }
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '%' | '=' => {
                let op: &'static str = match c {
                    '+' => "+",
                    '-' => "-",
                    '*' => "*",
                    '/' => "/",
                    '^' => "^",
                    '&' => "&",
                    '%' => "%",
                    _ => "=",
                };
                out.push(Tok::Op(op));
                i += 1;
            }
            _ => return Err("#PARSE!".to_string()),
        }
    }
    Ok(out)
}

// ───────────────────────────── Parser ─────────────────────────────

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Ref(Pos),
    Range(Pos, Pos),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Func(String, Vec<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

fn parse(src: &str) -> Result<Expr, String> {
    let toks = tokenize(src)?;
    let mut p = Parser { toks, pos: 0 };
    let e = p.cmp_expr()?;
    if p.pos < p.toks.len() {
        return Err("#PARSE!".to_string());
    }
    Ok(e)
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn eat_op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        if let Some(Tok::Op(o)) = self.peek() {
            if ops.contains(o) {
                let o = *o;
                self.pos += 1;
                return Some(o);
            }
        }
        None
    }

    fn cmp_expr(&mut self) -> Result<Expr, String> {
        let mut l = self.concat()?;
        while let Some(op) = self.eat_op(&["=", "<>", "<=", ">=", "<", ">"]) {
            let r = self.concat()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn concat(&mut self) -> Result<Expr, String> {
        let mut l = self.add()?;
        while let Some(op) = self.eat_op(&["&"]) {
            let r = self.add()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn add(&mut self) -> Result<Expr, String> {
        let mut l = self.mul()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let r = self.mul()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn mul(&mut self) -> Result<Expr, String> {
        let mut l = self.pow()?;
        while let Some(op) = self.eat_op(&["*", "/"]) {
            let r = self.pow()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    // Like Excel, unary minus binds tighter than ^ (so -2^2 = 4).
    fn pow(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        while let Some(op) = self.eat_op(&["^"]) {
            let r = self.unary()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if let Some(op) = self.eat_op(&["-", "+"]) {
            let e = self.unary()?;
            return Ok(if op == "-" { Expr::Neg(Box::new(e)) } else { e });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        while self.eat_op(&["%"]).is_some() {
            e = Expr::Pct(Box::new(e));
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::LParen) => {
                let e = self.cmp_expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err("#PARSE!".to_string()),
                }
            }
            Some(Tok::Ident(id)) => {
                if let Some(Tok::LParen) = self.peek() {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if let Some(Tok::RParen) = self.peek() {
                        self.pos += 1;
                        return Ok(Expr::Func(id.to_uppercase(), args));
                    }
                    loop {
                        args.push(self.cmp_expr()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            _ => return Err("#PARSE!".to_string()),
                        }
                    }
                    return Ok(Expr::Func(id.to_uppercase(), args));
                }
                let up = id.to_uppercase();
                if up == "TRUE" {
                    return Ok(Expr::Bool(true));
                }
                if up == "FALSE" {
                    return Ok(Expr::Bool(false));
                }
                match parse_cell_ref(&id) {
                    Some(p) => {
                        if let Some(Tok::Colon) = self.peek() {
                            self.pos += 1;
                            match self.next() {
                                Some(Tok::Ident(id2)) => match parse_cell_ref(&id2) {
                                    Some(p2) => Ok(Expr::Range(p, p2)),
                                    None => Err("#REF!".to_string()),
                                },
                                _ => Err("#PARSE!".to_string()),
                            }
                        } else {
                            Ok(Expr::Ref(p))
                        }
                    }
                    None => Err("#NAME?".to_string()),
                }
            }
            _ => Err("#PARSE!".to_string()),
        }
    }
}

// ───────────────────────────── Evaluator ─────────────────────────────

fn err(s: &str) -> Value {
    Value::Err(s.to_string())
}

fn to_num(v: &Value) -> Result<f64, Value> {
    match v {
        Value::Num(n) => Ok(*n),
        Value::Empty => Ok(0.0),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Text(s) => match s.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n),
            _ => Err(err("#VALUE!")),
        },
        Value::Err(_) => Err(v.clone()),
    }
}

fn to_bool(v: &Value) -> Result<bool, Value> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Num(n) => Ok(*n != 0.0),
        Value::Empty => Ok(false),
        Value::Text(s) => match s.to_uppercase().as_str() {
            "TRUE" => Ok(true),
            "FALSE" => Ok(false),
            _ => Err(err("#VALUE!")),
        },
        Value::Err(_) => Err(v.clone()),
    }
}

fn parse_literal(raw: &str) -> Value {
    let t = raw.trim();
    if t.is_empty() {
        return Value::Empty;
    }
    if let Ok(n) = t.parse::<f64>() {
        if n.is_finite() {
            return Value::Num(n);
        }
    }
    match t.to_uppercase().as_str() {
        "TRUE" => Value::Bool(true),
        "FALSE" => Value::Bool(false),
        _ => Value::Text(raw.to_string()),
    }
}

fn compare(a: &Value, b: &Value) -> Ordering {
    let (a, b) = match (a, b) {
        (Value::Empty, Value::Empty) => (Value::Num(0.0), Value::Num(0.0)),
        (Value::Empty, Value::Text(_)) => (Value::Text(String::new()), b.clone()),
        (Value::Text(_), Value::Empty) => (a.clone(), Value::Text(String::new())),
        (Value::Empty, Value::Bool(_)) => (Value::Bool(false), b.clone()),
        (Value::Bool(_), Value::Empty) => (a.clone(), Value::Bool(false)),
        (Value::Empty, _) => (Value::Num(0.0), b.clone()),
        (_, Value::Empty) => (a.clone(), Value::Num(0.0)),
        _ => (a.clone(), b.clone()),
    };
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Num(_) => 0,
            Value::Text(_) => 1,
            Value::Bool(_) => 2,
            _ => 3,
        }
    }
    match (&a, &b) {
        (Value::Num(x), Value::Num(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => rank(&a).cmp(&rank(&b)),
    }
}

struct Evaluator<'a> {
    sheet: &'a Sheet,
    cache: HashMap<Pos, Value>,
    visiting: HashSet<Pos>,
}

impl<'a> Evaluator<'a> {
    fn new(sheet: &'a Sheet) -> Self {
        Evaluator {
            sheet,
            cache: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    fn value(&mut self, pos: Pos) -> Value {
        if let Some(v) = self.cache.get(&pos) {
            return v.clone();
        }
        if self.visiting.contains(&pos) {
            return err("#CIRC!");
        }
        let raw = match self.sheet.cells.get(&pos) {
            Some(s) => s.clone(),
            None => return Value::Empty,
        };
        let v = if let Some(formula) = raw.strip_prefix('=') {
            self.visiting.insert(pos);
            let r = match parse(formula) {
                Ok(e) => self.eval(&e),
                Err(e) => Value::Err(e),
            };
            self.visiting.remove(&pos);
            r
        } else {
            parse_literal(&raw)
        };
        self.cache.insert(pos, v.clone());
        v
    }

    fn num(&mut self, e: &Expr) -> Result<f64, Value> {
        let v = self.eval(e);
        to_num(&v)
    }

    fn eval(&mut self, e: &Expr) -> Value {
        match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Text(s.clone()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Ref(p) => self.value(*p),
            Expr::Range(_, _) => err("#VALUE!"),
            Expr::Neg(x) => match self.num(x) {
                Ok(n) => Value::Num(-n),
                Err(e) => e,
            },
            Expr::Pct(x) => match self.num(x) {
                Ok(n) => Value::Num(n / 100.0),
                Err(e) => e,
            },
            Expr::Bin(op, l, r) => {
                let lv = self.eval(l);
                let rv = self.eval(r);
                if let Value::Err(_) = lv {
                    return lv;
                }
                if let Value::Err(_) = rv {
                    return rv;
                }
                match *op {
                    "&" => Value::Text(format!("{lv}{rv}")),
                    "+" | "-" | "*" | "/" | "^" => {
                        let a = match to_num(&lv) {
                            Ok(n) => n,
                            Err(e) => return e,
                        };
                        let b = match to_num(&rv) {
                            Ok(n) => n,
                            Err(e) => return e,
                        };
                        let res = match *op {
                            "+" => a + b,
                            "-" => a - b,
                            "*" => a * b,
                            "/" => {
                                if b == 0.0 {
                                    return err("#DIV/0!");
                                }
                                a / b
                            }
                            _ => a.powf(b),
                        };
                        if res.is_finite() {
                            Value::Num(res)
                        } else {
                            err("#NUM!")
                        }
                    }
                    _ => {
                        let ord = compare(&lv, &rv);
                        Value::Bool(match *op {
                            "=" => ord == Ordering::Equal,
                            "<>" => ord != Ordering::Equal,
                            "<" => ord == Ordering::Less,
                            ">" => ord == Ordering::Greater,
                            "<=" => ord != Ordering::Greater,
                            _ => ord != Ordering::Less,
                        })
                    }
                }
            }
            Expr::Func(name, args) => match self.func(name, args) {
                Ok(Value::Num(n)) if !n.is_finite() => err("#NUM!"),
                Ok(v) => v,
                Err(e) => e,
            },
        }
    }

    /// Flatten arguments into values. The bool marks values that came from a
    /// cell reference or range (where text/blank cells are ignored by SUM etc.).
    fn gather(&mut self, args: &[Expr]) -> Vec<(Value, bool)> {
        let mut out = Vec::new();
        let sheet = self.sheet;
        for a in args {
            match a {
                Expr::Range(p1, p2) => {
                    let (c1, c2) = (p1.0.min(p2.0), p1.0.max(p2.0));
                    let (r1, r2) = (p1.1.min(p2.1), p1.1.max(p2.1));
                    let mut keys: Vec<Pos> = sheet
                        .cells
                        .keys()
                        .filter(|&&(c, r)| c >= c1 && c <= c2 && r >= r1 && r <= r2)
                        .cloned()
                        .collect();
                    keys.sort_by_key(|&(c, r)| (r, c));
                    for k in keys {
                        let v = self.value(k);
                        if v != Value::Empty {
                            out.push((v, true));
                        }
                    }
                }
                Expr::Ref(p) => {
                    let v = self.value(*p);
                    out.push((v, true));
                }
                other => {
                    let v = self.eval(other);
                    out.push((v, false));
                }
            }
        }
        out
    }

    fn nums(&mut self, args: &[Expr]) -> Result<Vec<f64>, Value> {
        let mut out = Vec::new();
        for (v, from_ref) in self.gather(args) {
            match &v {
                Value::Err(_) => return Err(v.clone()),
                Value::Num(n) => out.push(*n),
                Value::Bool(_) | Value::Text(_) | Value::Empty if from_ref => {}
                other => out.push(to_num(other)?),
            }
        }
        Ok(out)
    }

    fn arg(&mut self, args: &[Expr], i: usize) -> Result<Value, Value> {
        match args.get(i) {
            Some(e) => Ok(self.eval(e)),
            None => Err(err("#N/A")),
        }
    }

    fn num_arg(&mut self, args: &[Expr], i: usize) -> Result<f64, Value> {
        let v = self.arg(args, i)?;
        to_num(&v)
    }

    fn text_arg(&mut self, args: &[Expr], i: usize) -> Result<String, Value> {
        let v = self.arg(args, i)?;
        if let Value::Err(_) = v {
            return Err(v);
        }
        Ok(v.to_string())
    }

    fn func(&mut self, name: &str, args: &[Expr]) -> Result<Value, Value> {
        Ok(match name {
            "SUM" => Value::Num(self.nums(args)?.iter().sum()),
            "PRODUCT" => Value::Num(self.nums(args)?.iter().product()),
            "AVERAGE" => {
                let n = self.nums(args)?;
                if n.is_empty() {
                    return Err(err("#DIV/0!"));
                }
                Value::Num(n.iter().sum::<f64>() / n.len() as f64)
            }
            "MIN" => {
                let n = self.nums(args)?;
                Value::Num(if n.is_empty() {
                    0.0
                } else {
                    n.iter().cloned().fold(f64::INFINITY, f64::min)
                })
            }
            "MAX" => {
                let n = self.nums(args)?;
                Value::Num(if n.is_empty() {
                    0.0
                } else {
                    n.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                })
            }
            "MEDIAN" => {
                let mut n = self.nums(args)?;
                if n.is_empty() {
                    return Err(err("#NUM!"));
                }
                n.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
                let m = n.len() / 2;
                Value::Num(if n.len() % 2 == 1 {
                    n[m]
                } else {
                    (n[m - 1] + n[m]) / 2.0
                })
            }
            "COUNT" => Value::Num(
                self.gather(args)
                    .iter()
                    .filter(|(v, _)| matches!(v, Value::Num(_)))
                    .count() as f64,
            ),
            "COUNTA" => Value::Num(
                self.gather(args)
                    .iter()
                    .filter(|(v, _)| !matches!(v, Value::Empty))
                    .count() as f64,
            ),
            "IF" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(err("#N/A"));
                }
                let c = self.eval(&args[0]);
                if to_bool(&c)? {
                    self.eval(&args[1])
                } else if args.len() == 3 {
                    self.eval(&args[2])
                } else {
                    Value::Bool(false)
                }
            }
            "IFERROR" => {
                if args.len() != 2 {
                    return Err(err("#N/A"));
                }
                let v = self.eval(&args[0]);
                if let Value::Err(_) = v {
                    self.eval(&args[1])
                } else {
                    v
                }
            }
            "AND" | "OR" => {
                let mut bs = Vec::new();
                for (v, from_ref) in self.gather(args) {
                    match &v {
                        Value::Err(_) => return Err(v.clone()),
                        Value::Empty | Value::Text(_) if from_ref => {}
                        other => bs.push(to_bool(other)?),
                    }
                }
                if bs.is_empty() {
                    return Err(err("#VALUE!"));
                }
                Value::Bool(if name == "AND" {
                    bs.iter().all(|b| *b)
                } else {
                    bs.iter().any(|b| *b)
                })
            }
            "NOT" => {
                let v = self.arg(args, 0)?;
                Value::Bool(!to_bool(&v)?)
            }
            "ABS" => Value::Num(self.num_arg(args, 0)?.abs()),
            "INT" => Value::Num(self.num_arg(args, 0)?.floor()),
            "SQRT" => {
                let x = self.num_arg(args, 0)?;
                if x < 0.0 {
                    return Err(err("#NUM!"));
                }
                Value::Num(x.sqrt())
            }
            "POWER" => {
                let a = self.num_arg(args, 0)?;
                let b = self.num_arg(args, 1)?;
                Value::Num(a.powf(b))
            }
            "MOD" => {
                let a = self.num_arg(args, 0)?;
                let b = self.num_arg(args, 1)?;
                if b == 0.0 {
                    return Err(err("#DIV/0!"));
                }
                Value::Num(a - b * (a / b).floor())
            }
            "ROUND" => {
                let x = self.num_arg(args, 0)?;
                let d = if args.len() > 1 { self.num_arg(args, 1)? } else { 0.0 };
                let f = 10f64.powi(d as i32);
                Value::Num((x * f).round() / f)
            }
            "PI" => Value::Num(std::f64::consts::PI),
            "LEN" => Value::Num(self.text_arg(args, 0)?.chars().count() as f64),
            "UPPER" => Value::Text(self.text_arg(args, 0)?.to_uppercase()),
            "LOWER" => Value::Text(self.text_arg(args, 0)?.to_lowercase()),
            "TRIM" => Value::Text(
                self.text_arg(args, 0)?
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            "LEFT" => {
                let s = self.text_arg(args, 0)?;
                let n = if args.len() > 1 {
                    self.num_arg(args, 1)?.max(0.0) as usize
                } else {
                    1
                };
                Value::Text(s.chars().take(n).collect())
            }
            "RIGHT" => {
                let s = self.text_arg(args, 0)?;
                let n = if args.len() > 1 {
                    self.num_arg(args, 1)?.max(0.0) as usize
                } else {
                    1
                };
                let cs: Vec<char> = s.chars().collect();
                let k = cs.len().saturating_sub(n);
                Value::Text(cs[k..].iter().collect())
            }
            "CONCAT" | "CONCATENATE" => {
                let mut s = String::new();
                for (v, _) in self.gather(args) {
                    if let Value::Err(_) = v {
                        return Err(v);
                    }
                    s.push_str(&v.to_string());
                }
                Value::Text(s)
            }
            _ => return Err(err("#NAME?")),
        })
    }
}

// ───────────────────────────── Tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(cells: &[(&str, &str)], at: &str) -> Value {
        let mut s = Sheet::default();
        for (a, v) in cells {
            s.set(parse_cell_ref(a).unwrap(), v);
        }
        s.evaluate_all()
            .get(&parse_cell_ref(at).unwrap())
            .cloned()
            .unwrap_or(Value::Empty)
    }

    #[test]
    fn addressing() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(parse_cell_ref("$B$3"), Some((1, 2)));
        assert_eq!(parse_cell_ref("A0"), None);
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval(&[("A1", "=1+2*3")], "A1"), Value::Num(7.0));
        assert_eq!(eval(&[("A1", "=(1+2)*3")], "A1"), Value::Num(9.0));
        assert_eq!(eval(&[("A1", "=2^3")], "A1"), Value::Num(8.0));
        assert_eq!(eval(&[("A1", "=-2^2")], "A1"), Value::Num(4.0));
        assert_eq!(eval(&[("A1", "=50%")], "A1"), Value::Num(0.5));
    }

    #[test]
    fn ranges_and_aggregates() {
        let c = [
            ("A1", "1"),
            ("A2", "2"),
            ("A3", "3"),
            ("B1", "=SUM(A1:A3)"),
            ("B2", "=AVERAGE(A1:A3)"),
            ("B3", "=MAX(A1:A3)"),
            ("B4", "=COUNT(A1:A3)"),
        ];
        assert_eq!(eval(&c, "B1"), Value::Num(6.0));
        assert_eq!(eval(&c, "B2"), Value::Num(2.0));
        assert_eq!(eval(&c, "B3"), Value::Num(3.0));
        assert_eq!(eval(&c, "B4"), Value::Num(3.0));
    }

    #[test]
    fn text_and_logic() {
        assert_eq!(eval(&[("A1", "=\"a\"&\"b\"")], "A1"), Value::Text("ab".into()));
        assert_eq!(eval(&[("A1", "=UPPER(\"abc\")")], "A1"), Value::Text("ABC".into()));
        assert_eq!(eval(&[("A1", "=LEFT(\"hello\",2)")], "A1"), Value::Text("he".into()));
        let c = [("A1", "5"), ("B1", "=IF(A1>3,\"big\",\"small\")")];
        assert_eq!(eval(&c, "B1"), Value::Text("big".into()));
    }

    #[test]
    fn errors() {
        assert_eq!(eval(&[("A1", "=1/0")], "A1"), Value::Err("#DIV/0!".into()));
        let c = [("A1", "=B1"), ("B1", "=A1")];
        assert_eq!(eval(&c, "A1"), Value::Err("#CIRC!".into()));
        assert_eq!(
            eval(&[("A1", "=IFERROR(1/0,\"oops\")")], "A1"),
            Value::Text("oops".into())
        );
    }

    #[test]
    fn csv_roundtrip() {
        let mut s = Sheet::default();
        s.set((0, 0), "a,b");
        s.set((1, 0), "say \"hi\"");
        s.set((0, 1), "=1+1");
        let back = Sheet::from_csv(&s.to_csv(), 26, 100);
        assert_eq!(s.cells, back.cells);
    }
}
