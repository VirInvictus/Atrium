with open("src/ui/theme.rs", "r") as f:
    lines = f.readlines()

new_lines = []
in_tests = False
for line in lines:
    if line.startswith("#[cfg(test)]"):
        in_tests = True
    
    if in_tests:
        if line == "}\n" and prev_line == "    }\n":
            in_tests = False
            continue
        prev_line = line
        continue

    # Remove the standard palette lines
    if "pub const BG_WINDOW:" in line or "pub const BG_VIEW:" in line or \
       "pub const BG_HEADER:" in line or "pub const BG_CARD:" in line or \
       "pub const FG:" in line or "pub const FG_DIM:" in line or \
       "pub const GRID:" in line or "pub const ACCENT:" in line or \
       "pub const ON_ACCENT:" in line or "pub const WARN:" in line or \
       "pub const ERR:" in line or "pub const OK:" in line:
        continue
    
    # Keep the swatch lines but make them non-pub
    if "pub const SW_" in line:
        line = line.replace("pub const SW_", "const SW_")
    
    # Replace sheet()
    if line.startswith("pub fn sheet() -> String {"):
        new_lines.append("""
pub fn sheet() -> String {
    let mut p = vir_gtk::theme::Palette::dragon();
    p.accent = "#c4b28a";
    p.on_accent = "#12120f";
    p.replace_tokens(TEMPLATE)
        .replace("%SW_BLUE%", SW_BLUE)
        .replace("%SW_GREEN%", SW_GREEN)
        .replace("%SW_YELLOW%", SW_YELLOW)
        .replace("%SW_ORANGE%", SW_ORANGE)
        .replace("%SW_RED%", SW_RED)
        .replace("%SW_PURPLE%", SW_PURPLE)
}

pub fn install() {
    vir_gtk::theme::install_stylesheet(&sheet());
}
""")
        break # Skip the rest since it's tests and install()
    
    new_lines.append(line)
    prev_line = line

with open("src/ui/theme.rs", "w") as f:
    f.writelines(new_lines)
