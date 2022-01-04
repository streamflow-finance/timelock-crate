#!/usr/bin/env python3
import re
import sys
from os.path import join
from subprocess import run

instruction_accounts = {
    "create": ("src/create.rs", "CreateAccounts"),
    "withdraw": ("src/withdraw.rs", "WithdrawAccounts"),
    "cancel": ("src/cancel.rs", "CancelAccounts"),
    "topup": ("src/topup.rs", "TopupAccounts"),
    "transfer": ("src/transfer.rs", "TransferAccounts"),
}

lifetimes = ["<'a>"]
structs = {}

header = """// Program interaction library
const borsh = require("borsh");
const spl = require("@solana/spl-token");

class Assignable {
    constructor(properties) {
        Object.keys(properties).map((key) => {
            this[key] = properties[key];
        });
    }
}
"""


def camel_to_snake(name):
    name = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', name).lower()


def lookup_layout(t, n):
    if t == "u32":
        return f"['{n}', 'u32'],"
    if t == "u64":
        return f"['{n}', 'u64'],"
    if t == "f32":
        return f"['{n}', 'f32'],"
    if t == "Pubkey":
        return f"['{n}', [32]],"
    if t == "bool" or t == "u8":
        return f"['{n}', 'u8'],"
    if t == "String":
        return f"['{n}', 'string'],"
    if t == "[u8; 64]":
        return f"['{n}', [64]],"
    if t == "[u8; MAX_NAME_SIZE_B]":
        return f"['{n}', [64]],"

    return None


def generate_schema(struct_name):
    for i in structs[struct_name]:
        bl = lookup_layout(i[1], i[0])

        if bl:
            print(f"        {bl}")
            continue

        if i[1] in structs:
            generate_schema(i[1])
            continue

        raise Exception(f"Unknown schema for {i[1]}")


def parse_structs(lines):
    found = False
    struct_name = None

    for i in lines:
        if found and not i.strip().startswith(
                "//") and not i.strip().startswith("}"):
            element = i.strip()[4:-1]

            # Remove Rust lifetimes
            for life in lifetimes:
                if element.endswith(life):
                    element = element[:-len(life)]

            elem_name, elem_type = element.split(": ")
            structs[struct_name].append((elem_name, elem_type))

        if i.startswith("pub struct"):
            found = True
            struct_name = i.split()[2]

            # Remove Rust lifetimes
            for life in lifetimes:
                if struct_name.endswith(life):
                    struct_name = struct_name[:-len(life)]

            structs[struct_name] = []

        if i.startswith("}"):
            found = False


def parse_instruction_accounts(lines, struct_name):
    found = False

    prog = re.compile("(?<=\[).+?(?=\])")
    accounts = []

    for idx, line in enumerate(lines):
        if found and not line.strip().startswith(
                "//") and not line.strip().startswith("}"):
            ln = line.strip()[4:]

            name = ln.split(": ")[0]
            attrs = ln.split("// ")[1][1:-1]

            # [mutable, signer]
            attr_tup = ["false", "false"]

            if attrs.strip():
                attrs = attrs.split(", ")
                if "writable" in attrs:
                    attr_tup[0] = "true"
                if "signer" in attrs:
                    attr_tup[1] = "true"

            accounts.append((name, attr_tup))

        if line.startswith("pub struct " + struct_name):
            found = True

        if line.startswith("}"):
            found = False

    for acc in accounts:
        print("      {")
        print(f'        "name": "{acc[0]}",')
        print(f'        "isMut": {acc[1][0]},')
        print(f'        "isSigner": {acc[1][1]},')
        print("      },")


def main():
    output = run(["git", "rev-parse", "--show-toplevel"], capture_output=True)
    toplevel = output.stdout.decode()[:-1]

    print(header)

    print("const program_instructions = [")
    for ix in instruction_accounts:
        print("  {")
        print(f"    // {ix} stream instruction")
        f = open(join(toplevel, instruction_accounts[ix][0]), "r")
        lines = f.readlines()
        f.close()

        print(f'    "name": "{ix}",')
        print('    "accounts": [')
        parse_instruction_accounts(lines, instruction_accounts[ix][1])
        print('    ]')
        print('  },')
    print('];\n')

    print("// data serialization")
    f = open(join(toplevel, "src/state.rs"), "r")
    lines = f.readlines()
    f.close()
    parse_structs(lines)

    for i in structs:
        print(f"class {i}Struct extends Assignable {{}}")
        print(f"const {i} = new Map([[{i}Struct, {{")
        print("    kind: 'struct',")
        print("    fields: [")
        generate_schema(i)
        print("    ]")
        print("}]]);\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
