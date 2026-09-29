fn main() {
    if let Err(error) = repowiki::cli::run() {
        repowiki::cli::print_error(&error);
        std::process::exit(1);
    }
}
