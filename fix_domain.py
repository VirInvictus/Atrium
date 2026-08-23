import re

with open("atrium-core/src/search/domain.rs", "r") as f:
    text = f.read()

text = text.replace("match s.to_ascii_lowercase().as_str()", "match name.to_ascii_lowercase().as_str()")

with open("atrium-core/src/search/domain.rs", "w") as f:
    f.write(text)
