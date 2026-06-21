{ pkgs, ... }:

{
  programs.git = {
    enable = true;
    package = pkgs.gitFull;
    settings = {
      user.name = "Christian Glassiognon";
      user.email = "63924603+heyglassy@users.noreply.github.com";
      user.signingKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIC/qhn9neDAsXF7tbLp+sao9P1YFq5/2pTIo5L/I5FFU";
      commit.gpgSign = true;
      gpg.format = "ssh";
      gpg.ssh.program = "/Applications/1Password.app/Contents/MacOS/op-ssh-sign";
      push.default = "simple";
      branch.autoSetupMerge = "simple";
      filter.lfs = {
        clean = "git-lfs clean -- %f";
        smudge = "git-lfs smudge -- %f";
        process = "git-lfs filter-process";
        required = true;
      };
      credential = {
        "https://github.com".helper = [
          ""
          "!/opt/homebrew/bin/gh auth git-credential"
        ];
        "https://gist.github.com".helper = [
          ""
          "!/opt/homebrew/bin/gh auth git-credential"
        ];
      };
    };
  };
}
