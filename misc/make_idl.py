#!/usr/bin/env python3
# This will generate a js lib to worh with struct serialization.
# It can be used as follows:
#
# const value = new Struct({
#     start_time: new BN(1234),
#     end_time: new BN(4321),
#     deposited_amount: new BN(31415),
#     total_amount: new BN(241519),
#     period: new BN(21515),
#     cliff: new BN(3184791),
#     cliff_amount: new BN(8241719),
#     cancelable_by_sender: true,
#     cancelable_by_recipent: false,
#     withdrawal_public: false,
#     transferable: false,
#     stream_name: "foobar",
# });
#
# const buffer = borsh.serialize(StreamInstruction, value);
# const de_value = borsh.deserialize(StreamInstruction, Struct, buffer);

import re
import sys
from os.path import join
from subprocess import run

files = ["src/state.rs"]
skips = [
    "InitializeAccounts", "WithdrawAccounts", "CancelAccounts",
    "TransferAccounts"
]
structs = {}

lifetimes = ["<'a>"]

header = """// Program interaction library
const borsh = require("borsh");

class Assignable {
    constructor(properties) {
        Object.keys(properties).map((key) => {
            this[key] = properties[key];
        });
    }
}

class Struct extends Assignable {}
"""


def camel_to_snake(name):
    name = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', name).lower()


def lookup_layout(t, n):
    if t == "u32":
        return f"['{n}', 'u32'],"
    if t == "u64":
        return f"['{n}', 'u64'],"
    if t == "Pubkey":
        #return f"['{n}', 'Uint8Array'],"
        return f"['{n}', [32]],"
    if t == "bool":
        return f"['{n}', 'u8'],"
    if t == "String":
        return f"['{n}', 'string'],"

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


def main():
    output = run(["git", "rev-parse", "--show-toplevel"], capture_output=True)
    toplevel = output.stdout.decode()[:-1]

    print(header)

    for src_file in files:
        f = open(join(toplevel, src_file), "r")
        lines = f.readlines()
        f.close()
        parse_structs(lines)

    for i in structs:
        if i in skips:
            continue

        print(f"const {i} = new Map([[Struct, {{")
        print("    kind: 'struct',")
        print("    fields: [")
        generate_schema(i)
        print("    ]")
        print("}]]);\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
