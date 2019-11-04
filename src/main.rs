use ureq::IpVersion;

fn main() {
    ureq::get("https://google.com/")
        .set_preferred_ip_version(IpVersion::V6)
        .call();
    println!("done");
}
