//! Minimal XML tree built on quick-xml, plus escaping for the writers.
//! Element and attribute names are stored without namespace prefixes.

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct XNode {
    /// Local name (without prefix), used for matching.
    pub name: String,
    /// Qualified name as written, used when writing the XML back.
    pub qname: String,
    /// Attributes as (qualified name, value).
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
    /// Text content for `#text` nodes.
    pub text: String,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl XNode {
    /// Attribute by local name.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| local(k) == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set an attribute by local name (added with the `w:` prefix if new).
    pub fn set_attr(&mut self, key: &str, val: impl Into<String>) {
        let val = val.into();
        match self.attrs.iter_mut().find(|(k, _)| local(k) == key) {
            Some((_, v)) => *v = val,
            None => self.attrs.push((format!("w:{key}"), val)),
        }
    }

    pub fn remove_attr(&mut self, key: &str) {
        self.attrs.retain(|(k, _)| local(k) != key);
    }

    /// A new empty `w:` element.
    pub fn elem(local_name: &str) -> XNode {
        XNode {
            name: local_name.into(),
            qname: format!("w:{local_name}"),
            ..Default::default()
        }
    }

    pub fn attrs_xml(&self) -> String {
        self.attrs
            .iter()
            .map(|(k, v)| format!(" {k}=\"{}\"", esc(v)))
            .collect()
    }

    pub fn start_tag(&self) -> String {
        format!("<{}{}>", self.qname, self.attrs_xml())
    }

    pub fn end_tag(&self) -> String {
        format!("</{}>", self.qname)
    }

    /// Serialize this element (and everything inside it).
    pub fn to_xml(&self) -> String {
        let mut s = String::new();
        self.write_xml(&mut s);
        s
    }

    fn write_xml(&self, out: &mut String) {
        if self.name == "#text" {
            out.push_str(&esc(&self.text));
            return;
        }
        if self.children.is_empty() {
            out.push_str(&format!("<{}{}/>", self.qname, self.attrs_xml()));
            return;
        }
        out.push_str(&self.start_tag());
        for c in &self.children {
            c.write_xml(out);
        }
        out.push_str(&self.end_tag());
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut XNode> {
        self.children.iter_mut().find(|c| c.name == name)
    }

    pub fn child(&self, name: &str) -> Option<&XNode> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XNode> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// First descendant with this name (depth-first).
    pub fn find(&self, name: &str) -> Option<&XNode> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(f) = c.find(name) {
                return Some(f);
            }
        }
        None
    }

    /// All text below this node.
    pub fn all_text(&self) -> String {
        let mut s = String::new();
        self.collect_text(&mut s);
        s
    }

    fn collect_text(&self, out: &mut String) {
        if self.name == "#text" {
            out.push_str(&self.text);
        }
        for c in &self.children {
            c.collect_text(out);
        }
    }

    fn push_text(&mut self, t: &str) {
        if let Some(last) = self.children.last_mut()
            && last.name == "#text"
        {
            last.text.push_str(t);
            return;
        }
        self.children.push(XNode {
            name: "#text".into(),
            text: t.to_string(),
            ..Default::default()
        });
    }
}

fn resolve_entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let v = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(v);
    }
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

pub fn parse(s: &str) -> Result<XNode, String> {
    let mut r = Reader::from_str(s);
    let mut stack = vec![XNode {
        name: "#root".into(),
        ..Default::default()
    }];
    let node_from = |e: &quick_xml::events::BytesStart| XNode {
        name: local(e.name().0).to_string(),
        qname: e.name().0.to_string(),
        attrs: e
            .attributes()
            .filter_map(Result::ok)
            .map(|a| {
                let v = a
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_else(|_| a.value.to_string());
                (a.key.0.to_string(), v)
            })
            .collect(),
        ..Default::default()
    };
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => stack.push(node_from(&e)),
            Ok(Event::Empty(e)) => {
                let n = node_from(&e);
                stack.last_mut().unwrap().children.push(n);
            }
            Ok(Event::End(_)) => {
                let n = stack.pop().unwrap();
                match stack.last_mut() {
                    Some(parent) => parent.children.push(n),
                    None => return Err("unbalanced XML".into()),
                }
            }
            Ok(Event::Text(t)) => stack.last_mut().unwrap().push_text(&t.xml10_content()),
            Ok(Event::CData(t)) => stack.last_mut().unwrap().push_text(&t.xml10_content()),
            Ok(Event::GeneralRef(e)) => {
                if let Some(c) = resolve_entity(&e.xml10_content()) {
                    stack.last_mut().unwrap().push_text(&c.to_string());
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML error: {e}")),
            _ => {}
        }
    }
    while stack.len() > 1 {
        let n = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(n);
    }
    Ok(stack.pop().unwrap())
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\r' => o.push_str("&#13;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' => {}
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entities_and_prefixes() {
        let x = parse(r#"<w:p a:x="1 &amp; 2"><w:t>a &lt; b &#x41;</w:t></w:p>"#).unwrap();
        let p = x.child("p").unwrap();
        assert_eq!(p.attr("x"), Some("1 & 2"));
        assert_eq!(p.all_text(), "a < b A");
        assert_eq!(
            p.to_xml(),
            r#"<w:p a:x="1 &amp; 2"><w:t>a &lt; b A</w:t></w:p>"#
        );
    }
}
