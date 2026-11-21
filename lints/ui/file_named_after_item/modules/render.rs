pub fn page(title: &str) -> String {
    Buffer(title.to_owned()).0
}

pub fn table(rows: &[&str]) -> String {
    Buffer(rows.concat()).0
}

struct Buffer(String);
