import re

with open('src/lua/engine.rs', 'r') as f:
    content = f.read()

# Find all AddTrigger lines with 2049 that should be 33
# AddTrigger uses trigger_flag where RegularExpression=32, so 33 = 1+32 is correct
lines = content.split('\n')
fixed = 0
for i, line in enumerate(lines):
    if 'AddTrigger' in line and '2049' in line:
        # Replace 2049 with 33 in AddTrigger flags position
        old = line
        # Be careful: only replace the first occurrence of 2049 in the line
        # (the flags parameter, not other numbers)
        line = line.replace('', 2049, 0, 0', '', 33, 0, 0', 1)
        if line != old:
            fixed += 1
        lines[i] = line

content = '\n'.join(lines)

with open('src/lua/engine.rs', 'w') as f:
    f.write(content)

print(f'Fixed {fixed} AddTrigger lines')
