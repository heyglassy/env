{ ... }:

{
  xdg.configFile."mise/conf.d/rust.toml".text = ''
    [tools]
    rust = "1.96.0"
  '';
}
