# dmgbuild settings for the OpenVClient installer disk image.
# Used by scripts/package-macos-dmg.sh; `defines` come from the command line:
#   -D app=dist/OpenVClient.app -D root=<repo root>
import os

app = defines.get("app", "dist/OpenVClient.app")  # noqa: F821 (provided by dmgbuild)
root = defines.get("root", ".")  # noqa: F821
app_name = os.path.basename(app)

format = "ULFO"  # LZFSE compressed, macOS 10.11+
filesystem = "APFS"
files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = [app_name]

badge_icon = None
icon = os.path.join(root, "assets", "icon", "openvclient.icns")  # volume icon

# Finder window: 660x420, background with the drag arrow, two large icons.
background = os.path.join(root, "assets", "installer", "dmg-background.png")
window_rect = ((200, 160), (660, 420))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 112
text_size = 13
icon_locations = {app_name: (170, 230), "Applications": (490, 230)}
arrange_by = None
grid_offset = (0, 0)
label_pos = "bottom"

license = None
