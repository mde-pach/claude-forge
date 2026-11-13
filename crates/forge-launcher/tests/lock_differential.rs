//! The fast lock scanner must agree with a full TOML parser on `forge`
//! for every valid document.

use forge_launcher::parse_lock;

fn reference(text: &str) -> Option<String> {
    let v: toml::Table = text.parse().expect("sample must be valid TOML");
    v.get("forge").and_then(|f| f.as_str()).map(str::to_owned)
}

const PIECES: &[&str] = &[
    "schema = 1\n",
    "# comment [x]\n",
    "\n",
    "name = \"a \\\" ] [ b\"\n",
    "lit = 'C:\\path'\n",
    "multi = \"\"\"\n[fake]\nforge = \"9.9.9\"\n\"\"\"\n",
    "mlit = '''\nforge = '8.8.8'\n'''\n",
    "arr = [\n  \"[x]\", # ]\n  [1, 2],\n  { a = \"}\" },\n]\n",
    "inline = { forge = \"7.7.7\", n = [1] }\n",
    "when = 1979-05-27 07:32:00Z\n",
    "dotted.key = true\n",
    "\"quoted key\" = 1.5\n",
    "neg = -3 # trailing\n",
    "crlf = 1\r\n",
];

#[test]
fn agrees_with_toml_crate() {
    let forge_lines = [
        "forge = \"0.7.0\"\n",
        "forge = '1.0.0-rc.1'  # pin\n",
        "\"forge\" = \"2.0.0+b\"\n",
    ];
    let tails = [
        "",
        "[packs]\nforge = \"5.5.5\"\n",
        "[[tools]]\nname = \"uv\"\n",
    ];
    let mut n = 0;
    // Every rotation of the pieces, with the forge line at every position
    // (or absent), followed by every tail.
    for rot in 0..PIECES.len() {
        let pieces: Vec<&str> = PIECES
            .iter()
            .cycle()
            .skip(rot)
            .take(PIECES.len())
            .copied()
            .collect();
        for at in 0..=pieces.len() {
            for forge in forge_lines.iter().map(Some).chain([None]) {
                for tail in tails {
                    let mut doc = String::new();
                    for (i, p) in pieces.iter().enumerate() {
                        if i == at {
                            doc.extend(forge.copied());
                        }
                        doc.push_str(p);
                    }
                    if at == pieces.len() {
                        doc.extend(forge.copied());
                    }
                    doc.push_str(tail);
                    let got = parse_lock(&doc)
                        .unwrap_or_else(|e| panic!("{e}\n---\n{doc}"))
                        .forge;
                    assert_eq!(got, reference(&doc), "\n---\n{doc}");
                    n += 1;
                }
            }
        }
    }
    assert!(n > 1000);
}
