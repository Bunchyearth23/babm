//! Read JBeam's JSON dialect without evaluating Lua expressions.
use serde_json::{Map, Value};

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        pos: 0,
    };
    let value = p.value(0)?;
    p.space()?;
    while p.s.get(p.pos) == Some(&b',') {
        p.pos += 1;
        p.space()?;
    }
    if p.pos != p.s.len() {
        return Err(format!("Trailing JBeam data at byte {}", p.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}
impl Parser<'_> {
    fn space(&mut self) -> Result<(), String> {
        loop {
            while self.s.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
                self.pos += 1;
            }
            if self.s.get(self.pos..self.pos + 2) == Some(b"//") {
                while self.s.get(self.pos).is_some_and(|c| *c != b'\n') {
                    self.pos += 1;
                }
            } else if self.s.get(self.pos..self.pos + 2) == Some(b"/*") {
                self.pos += 2;
                while self.pos + 1 < self.s.len()
                    && self.s.get(self.pos..self.pos + 2) != Some(b"*/")
                {
                    self.pos += 1;
                }
                if self.pos + 1 >= self.s.len() {
                    return Err("Unterminated JBeam comment".into());
                }
                self.pos += 2;
            } else {
                return Ok(());
            }
        }
    }
    fn string(&mut self) -> Result<String, String> {
        let start = self.pos;
        self.pos += 1;
        loop {
            match self.s.get(self.pos) {
                Some(b'"') => {
                    self.pos += 1;
                    return serde_json::from_slice(&self.s[start..self.pos])
                        .map_err(|e| e.to_string());
                }
                Some(b'\\') => self.pos += 2,
                Some(_) => self.pos += 1,
                None => return Err("Unterminated JBeam string".into()),
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 96 {
            return Err("JBeam nesting limit exceeded".into());
        }
        self.space()?;
        match self.s.get(self.pos).copied() {
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b'{') => {
                self.pos += 1;
                let mut object = Map::new();
                loop {
                    self.space()?;
                    while self.s.get(self.pos) == Some(&b',') {
                        self.pos += 1;
                        self.space()?;
                    }
                    if self.s.get(self.pos) == Some(&b'}') {
                        self.pos += 1;
                        return Ok(Value::Object(object));
                    }
                    if self.s.get(self.pos) != Some(&b'"') {
                        return Err(format!("Expected JBeam key at {}", self.pos));
                    }
                    let key = self.string()?;
                    self.space()?;
                    if self.s.get(self.pos) != Some(&b':') {
                        return Err("Missing JBeam colon".into());
                    }
                    self.pos += 1;
                    // BeamNG's reader accepts duplicate dictionary fields; the last wins.
                    object.insert(key, self.value(depth + 1)?);
                    self.space()?;
                    if self.s.get(self.pos) == Some(&b',') {
                        self.pos += 1;
                    }
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let mut array = Vec::new();
                loop {
                    self.space()?;
                    if self.s.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        return Ok(Value::Array(array));
                    }
                    array.push(self.value(depth + 1)?);
                    self.space()?;
                    if self.s.get(self.pos) == Some(&b',') {
                        self.pos += 1;
                    }
                }
            }
            Some(_) => {
                let start = self.pos;
                while self
                    .s
                    .get(self.pos)
                    .is_some_and(|c| !c.is_ascii_whitespace() && !b",]}:/".contains(c))
                {
                    self.pos += 1;
                }
                if start == self.pos {
                    return Err(format!("Invalid JBeam token at {}", self.pos));
                }
                let token = &self.s[start..self.pos];
                if token == b"FLT_MAX" {
                    return Ok(Value::String("FLT_MAX".into()));
                }
                serde_json::from_slice(token).map_err(|e| format!("Invalid JBeam token: {e}"))
            }
            None => Err("Unexpected end of JBeam".into()),
        }
    }
}
