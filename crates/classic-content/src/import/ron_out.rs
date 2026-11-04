//! RON text writer of the importer: one record per block, one field per
//! line, default values left out (the parser defaults them), lists of
//! records one item per line. Output depends only on the input order, which
//! the importer sorts by key.

/// One RON record `( field: value, ... )` in insertion order.
#[derive(Debug, Default, Clone)]
pub struct Record {
    fields: Vec<(&'static str, Value)>,
}

#[derive(Debug, Clone)]
enum Value {
    Inline(String),
    /// A list rendered one item per line.
    Lines(Vec<String>),
}

pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Record {
    pub fn new() -> Self {
        Self::default()
    }

    /// A field written as is (already RON).
    pub fn raw(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.fields.push((name, Value::Inline(value.into())));
        self
    }

    pub fn str(self, name: &'static str, value: &str) -> Self {
        self.raw(name, quote(value))
    }

    /// Left out when empty.
    pub fn text(self, name: &'static str, value: &str) -> Self {
        if value.is_empty() {
            return self;
        }
        self.str(name, value)
    }

    /// `Some("...")`, left out when None.
    pub fn opt_str(self, name: &'static str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.raw(name, format!("Some({})", quote(v))),
            None => self,
        }
    }

    /// Left out when zero.
    pub fn num(self, name: &'static str, value: i64) -> Self {
        if value == 0 {
            return self;
        }
        self.raw(name, value.to_string())
    }

    /// Left out when false.
    pub fn flag(self, name: &'static str, value: bool) -> Self {
        if !value {
            return self;
        }
        self.raw(name, "true")
    }

    /// `["a", "b"]`, left out when empty.
    pub fn strs(self, name: &'static str, values: &[String]) -> Self {
        if values.is_empty() {
            return self;
        }
        let items: Vec<String> = values.iter().map(|v| quote(v)).collect();
        self.raw(name, format!("[{}]", items.join(", ")))
    }

    /// A list of RON items one per line, left out when empty.
    pub fn lines(mut self, name: &'static str, items: Vec<String>) -> Self {
        if items.is_empty() {
            return self;
        }
        self.fields.push((name, Value::Lines(items)));
        self
    }

    /// Single-line rendering `(a: 1, b: "x")`.
    pub fn inline(&self) -> String {
        let mut parts = Vec::with_capacity(self.fields.len());
        for (name, value) in &self.fields {
            let text = match value {
                Value::Inline(v) => v.clone(),
                Value::Lines(items) => format!("[{}]", items.join(", ")),
            };
            parts.push(format!("{name}: {text}"));
        }
        format!("({})", parts.join(", "))
    }

    fn block(&self, indent: &str) -> String {
        let inner = format!("{indent}    ");
        let mut out = format!("{indent}(\n");
        for (name, value) in &self.fields {
            match value {
                Value::Inline(v) => out.push_str(&format!("{inner}{name}: {v},\n")),
                Value::Lines(items) => {
                    out.push_str(&format!("{inner}{name}: [\n"));
                    for item in items {
                        out.push_str(&format!("{inner}    {item},\n"));
                    }
                    out.push_str(&format!("{inner}],\n"));
                }
            }
        }
        out.push_str(&format!("{indent}),\n"));
        out
    }
}

/// A ruleset file: a header comment and named tables of records.
pub fn file(header: &str, tables: &[(&str, &[Record])]) -> String {
    let mut out = String::new();
    for line in header.lines() {
        out.push_str("// ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("(\n");
    for (name, records) in tables {
        out.push_str(&format!("    {name}: [\n"));
        for record in records.iter() {
            out.push_str(&record.block("        "));
        }
        out.push_str("    ],\n");
    }
    out.push_str(")\n");
    out
}

/// A ruleset file holding one record `name: (...)`.
pub fn record_file(header: &str, name: &str, record: &Record) -> String {
    let mut out = String::new();
    for line in header.lines() {
        out.push_str("// ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("(\n");
    let block = record.block("    ");
    out.push_str(&format!("    {name}: {}", block.trim_start()));
    out.push_str(")\n");
    out
}
