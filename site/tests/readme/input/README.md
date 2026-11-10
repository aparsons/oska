# Oska

A language where `main` gets only the capabilities it declares & nothing else.

```oska
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("Hello <world>!")?
    Ok(())
}
```
