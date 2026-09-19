fn main() {
    if let Err(error) = codewiki::cli::run() {
        codewiki::cli::print_error(&error);
        std::process::exit(1);
    }
}
