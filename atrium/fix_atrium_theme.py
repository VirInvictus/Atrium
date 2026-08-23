import re
with open("src/ui/theme.rs", "r") as f:
    text = f.read()

# Replace the palette constants
text = re.sub(r'pub const BG_WINDOW:.*?OK:.*?;\n', '', text, flags=re.DOTALL)
text = text.replace('pub const SW_BLUE', 'const SW_BLUE')
text = text.replace('pub const SW_GREEN', 'const SW_GREEN')
text = text.replace('pub const SW_YELLOW', 'const SW_YELLOW')
text = text.replace('pub const SW_ORANGE', 'const SW_ORANGE')
text = text.replace('pub const SW_RED', 'const SW_RED')
text = text.replace('pub const SW_PURPLE', 'const SW_PURPLE')

# Update sheet()
new_sheet = """
pub fn sheet() -> String {
    let mut p = vir_gtk::theme::Palette::dragon();
    p.accent = "#c4b28a"; // Atrium uses dragonYellow as accent
    p.on_accent = "#12120f";
    p.replace_tokens(TEMPLATE)
        .replace("%SW_BLUE%", SW_BLUE)
        .replace("%SW_GREEN%", SW_GREEN)
        .replace("%SW_YELLOW%", SW_YELLOW)
        .replace("%SW_ORANGE%", SW_ORANGE)
        .replace("%SW_RED%", SW_RED)
        .replace("%SW_PURPLE%", SW_PURPLE)
}
"""
text = re.sub(r'pub fn sheet\(\) -> String \{.*?\n\}', new_sheet.strip(), text, flags=re.DOTALL)

# Update install()
new_install = """
pub fn install() {
    vir_gtk::theme::install_stylesheet(&sheet());
}
"""
text = re.sub(r'pub fn install\(\) \{.*?\n\}', new_install.strip(), text, flags=re.DOTALL)

with open("src/ui/theme.rs", "w") as f:
    f.write(text)
