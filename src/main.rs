use std::error::Error;
fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init_timed();
    rotterdam::main()
}
