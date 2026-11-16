# Sourced by the release and deploy scripts: where the packages and the website go.
#
#   PKG_HOST    SSH target of the server (user@host or a name from ~/.ssh/config)
#   PKG_REMOTE  web root of pkg.wolkenfunk.org there (default /root/claudes/wolkenfunk/pkg)
#
# Both come from the environment or from the .env file in the repository root (another file
# with WOLKENFUNK_ENV). The server is not named in this repository; without PKG_HOST the
# script stops instead of guessing.
_env="${WOLKENFUNK_ENV:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.env}"
for _key in PKG_HOST PKG_REMOTE; do
  [ -n "${!_key:-}" ] && continue
  _line=$(grep -E "^$_key=" "$_env" 2>/dev/null | tail -n1) || continue
  _line="${_line#*=}"; _line="${_line%\"}"; _line="${_line#\"}"
  [ -n "$_line" ] && export "$_key=$_line"
done
unset _env _key _line
: "${PKG_HOST:?set PKG_HOST (SSH target of the package server) in the environment or in .env}"
export PKG_HOST PKG_REMOTE="${PKG_REMOTE:-/root/claudes/wolkenfunk/pkg}"
