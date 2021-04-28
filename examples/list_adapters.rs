#[tokio::main(flavor = "current_thread")]
async fn main() -> blurz::Result<()> {
    let session = blurz::Session::new().await?;
    let adapter_names = session.adapter_names().await?;
    for adapter_name in adapter_names {
        println!("Bluetooth adapater {}:", &adapter_name);
        let adapter = session.adapter(&adapter_name)?;
        println!("    Address:         {}", adapter.address().await?);
        println!("    Address type:    {}", adapter.address_type().await?);
        println!("    Friendly name:   {}", adapter.alias().await?);
        println!(
            "    Discoverabe:     {:?}",
            adapter.is_discoverable().await?
        );
        println!();
    }
    Ok(())
}
