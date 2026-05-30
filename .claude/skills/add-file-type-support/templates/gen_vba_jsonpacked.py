#!/usr/bin/env python3
"""Generate synthetic VBA+JSONPacked training samples.

Mirrors the structure of multi-module VBA archives that some extractors
emit: a JSON object whose keys are module names and whose values are
strings containing the module's full source code (with `\\r\\n` and
`\\"` JSON-escapes intact).
"""
import json
import os
import random
import string

ROOT = "/Users/dazhi/projects/filetyping/whatis/samples/VBA+JSONPacked"

EXCEL_WORKBOOK_GUID = "0{00020820-0000-0000-C000-000000000046}"

NORMAL_NAMES = [
    "ThisWorkbook", "Module1", "Module2", "Module3", "Module4",
    "Sheet1", "Sheet2", "Sheet3", "Sheet4", "Sheet5",
    "UserForm1", "UserForm2", "frmMain", "frmConfig",
    "clsUtil", "clsData", "clsLogger", "modUtilities", "modMain",
    "Hcompra1", "Hventa1", "Hdiario1", "Hplan1", "HMayor",
]
SPANISH_NAMES = [
    "Módulo1", "Módulo2", "Módulo3", "Hoja1", "Hoja2", "Hoja3",
    "ThisWorkbook", "Libro1", "Calculo", "Reportes",
]


def junk_name():
    # Mimic the obfuscated module-name style observed in real samples:
    # short alpha-digit clusters concatenated with no spaces.
    parts = []
    for _ in range(random.randint(4, 12)):
        parts.append(random.choice(string.ascii_lowercase) + str(random.randint(0, 9)))
    return "".join(parts)


def ident(n_range=(3, 12)):
    n = random.randint(*n_range)
    return random.choice(string.ascii_letters) + "".join(
        random.choices(string.ascii_letters + string.digits + "_", k=n - 1)
    )


def header_block(name, full=True):
    if full:
        return (
            f'Attribute VB_Name = "{name}"\r\n'
            f'Attribute VB_Base = "{EXCEL_WORKBOOK_GUID}"\r\n'
            f"Attribute VB_GlobalNameSpace = False\r\n"
            f"Attribute VB_Creatable = False\r\n"
            f"Attribute VB_PredeclaredId = True\r\n"
            f"Attribute VB_Exposed = True\r\n"
            f"Attribute VB_TemplateDerived = False\r\n"
            f"Attribute VB_Customizable = True\r\n"
        )
    return f'Attribute VB_Name = "{name}"\r\n'


SUB_TEMPLATES = [
    'Private Sub {n}_Click()\r\n  Dim {v} As Integer\r\n  {v} = {k}\r\n  MsgBox "{m}", vbInformation\r\nEnd Sub\r\n',
    'Public Sub {n}()\r\n  Dim ws As Worksheet\r\n  Set ws = Worksheets("{m}")\r\n  ws.Range("A{k}").Value = "{m}"\r\n  ws.Cells({k}, 2).Value = Now\r\nEnd Sub\r\n',
    'Private Sub {n}_Change()\r\n  If Me.{v}.Value <> "" Then\r\n    Me.{v}.BackColor = RGB({k}, {k2}, {k3})\r\n  End If\r\nEnd Sub\r\n',
    'Sub {n}()\r\n  Dim i As Long, total As Double\r\n  total = 0\r\n  For i = 1 To {k}\r\n    total = total + Cells(i, 1).Value\r\n  Next i\r\n  Range("B{k}").Value = total\r\nEnd Sub\r\n',
    'Public Function {n}({v} As String) As String\r\n  {n} = UCase(Trim({v}))\r\nEnd Function\r\n',
    'Private Sub {n}_Initialize()\r\n  Me.Caption = "{m}"\r\n  Me.BackColor = RGB({k}, {k2}, {k3})\r\n  Call Refresh{v}\r\nEnd Sub\r\n',
    'Public Sub {n}()\r\n  Dim wb As Workbook\r\n  Set wb = ActiveWorkbook\r\n  wb.Sheets("{m}").Activate\r\n  ActiveSheet.Range("A1:D{k}").ClearContents\r\nEnd Sub\r\n',
]


def random_sub():
    tpl = random.choice(SUB_TEMPLATES)
    return tpl.format(
        n=ident(),
        v=ident(),
        k=random.randint(1, 999),
        k2=random.randint(0, 255),
        k3=random.randint(0, 255),
        m=ident((4, 16)),
    )


def normal_module_body(name):
    full = name in ("ThisWorkbook",) or name.startswith(("Sheet", "User", "frm"))
    parts = [header_block(name, full=full)]
    parts.append("Option Explicit\r\n\r\n")
    for _ in range(random.randint(2, 12)):
        parts.append(random_sub())
        parts.append("\r\n")
    return "".join(parts)


def junk_module_body(name):
    # Mimic the obfuscated style: header + many lines of inscrutable
    # property accesses with junk identifiers, mostly Sub-shaped.
    parts = [header_block(name, full=True)]
    parts.append("Option Explicit\r\n\r\n")
    for _ in range(random.randint(3, 25)):
        sub_name = junk_name()
        parts.append(f"Private Sub {sub_name}()\r\n")
        for _ in range(random.randint(5, 40)):
            lhs = junk_name()
            rhs = junk_name()
            parts.append(f"  {lhs} = {rhs}\r\n")
        parts.append("End Sub\r\n\r\n")
    return "".join(parts)


def gen_archive(seed):
    random.seed(seed)
    n_modules = random.randint(5, 120)
    obj = {}
    # ThisWorkbook is almost always present
    if random.random() < 0.8:
        obj["ThisWorkbook"] = normal_module_body("ThisWorkbook")
    n_normal = random.randint(1, max(2, n_modules // 4))
    n_junk = max(0, n_modules - n_normal - 1)

    used_names = set(obj.keys())
    name_pool = NORMAL_NAMES + SPANISH_NAMES
    random.shuffle(name_pool)
    for name in name_pool[:n_normal]:
        if name in used_names:
            name = name + str(random.randint(1, 99))
        used_names.add(name)
        obj[name] = normal_module_body(name)
    for _ in range(n_junk):
        name = junk_name()
        if name in used_names:
            continue
        used_names.add(name)
        obj[name] = junk_module_body(name)
    return obj


def main():
    os.makedirs(ROOT, exist_ok=True)
    for i in range(12):
        archive = gen_archive(seed=4000 + i)
        path = os.path.join(ROOT, f"vba_jsonpacked_{i}.json")
        with open(path, "w") as f:
            json.dump(archive, f)
        print(f"wrote {path} ({os.path.getsize(path)} bytes, {len(archive)} modules)")


if __name__ == "__main__":
    main()
