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

    print("program_instructions: [")

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

    print(']')

    return 0


if __name__ == "__main__":
    sys.exit(main())
