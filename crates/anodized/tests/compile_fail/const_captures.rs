use anodized::spec;

#[spec(captures: before = value, ensures: |output| output == before)]
const fn invalid(value: u32) -> u32 {
    value
}

fn main() {}
