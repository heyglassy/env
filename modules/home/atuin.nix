{ ... }:

{
  programs.atuin = {
    enable = true;
    enableBashIntegration = false;
    settings = {
      auto_sync = true;
      enter_accept = true;
      filter_mode_shell_up_key_binding = "directory";
    };
  };
}
