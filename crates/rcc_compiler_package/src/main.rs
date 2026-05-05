use rcc_driver::Cli;

fn main() {
    let cli = Cli::parse();
    let code = rcc_driver::run(cli);
    std::process::exit(code);
}
