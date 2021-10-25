#!/usr/bin/env python3
# A really lazy parser
import sys
import re
from os.path import join
from subprocess import run

files = ["src/state.rs"]
skips = [
    "InitializeAccounts", "WithdrawAccounts", "CancelAccounts",
    "TransferAccounts"
]
structs = {}


def camel_to_snake(name):
    name = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', name).lower()


def lookup_layout(t, n):
    if t == "u64":
        return f'BufferLayout.blob(8, "{n}"),'
    if t == "Pubkey":
        return f'BufferLayout.blob(32, "{n}"),'
    if t == "bool":
        return f'BufferLayout.blob(1, "{n}"),'
    if t == "String":
        return f'BufferLayout.blob(1, "{n}"),'#TODO: String length
    if t == "u32":
        return f'BufferLayout.blob(4, "{n}"),'

    return None


def generate_buflayout(struct_name):
    for i in structs[struct_name]:
        bl = lookup_layout(i[1], i[0])
        if bl:
            print(f"    {bl}")
            continue
        if i[1] in structs:
            generate_buflayout(i[1])
            continue

        raise Exception(f"Unknown buffer layout for {i[1]})")


def lookup_decoder(t, n):
    if t == "u64":
        return f'"{n}": new BN(raw.{n}, LE),'
    if t == "Pubkey":
        return f'"{n}": new PublicKey(raw.{n}),'
    if t == "bool":
        return f'"{n}": Boolean(raw.{n}.readUInt8()),'
    if t == "String":
        return f'"{n}": String(raw.{n}),'
    if t == "u32":
        return " " #skip the padding.

    return None


def generate_decoder(struct_name):
    for i in structs[struct_name]:
        dl = lookup_decoder(i[1], i[0])
        if dl:
            print(f"        {dl}")
            continue
        if i[1] in structs:
            generate_decoder(i[1])
            continue

        raise Exception(f"Unknown decoder for {i[1]}")

def lookup_interface(t, n):
    if t == "u64":
        return f'{n}: BN;'
    if t == "Pubkey":
        return f'{n}: PublicKey;'
    if t == "bool":
        return f'{n}: boolean;'
    if t == "String":
        return f'{n}: string;'
    if t == "u32":
        return " " #skip the padding

    return None

def generate_interface(struct_name):
    for i in structs[struct_name]:
        dl = lookup_interface(i[1], i[0])
        if dl:
            print(f"  {dl}")
            continue
        if i[1] in structs:
            generate_interface(i[1])
            continue

        raise Exception(f"Unknown interface type for {i[1]}")

def parse_structs(lines):
    found = False
    struct_name = None

    for i in lines:
        if found and not i.strip().startswith(
                "//") and not i.strip().startswith("}"):
            element = i.strip()[4:-1]
            if element.endswith("<'a>"):
                element = element[:-4]

            elem_name, elem_type = element.split(": ")
            structs[struct_name].append((elem_name, elem_type))

        if i.startswith("pub struct"):
            found = True
            struct_name = i.split()[2]
            if struct_name.endswith("<'a>"):
                struct_name = struct_name[:-4]
            structs[struct_name] = []

        if i.startswith("}"):
            found = False


def main():
    output = run(["git", "rev-parse", "--show-toplevel"], capture_output=True)
    toplevel = output.stdout.decode()[:-1]

    print("const BufferLayout = require('buffer-layout');")
    print("const { PublicKey } = require('@solana/web3.js');")
    print("const anchor = require('@project-serum/anchor');")
    print("const { BN } = anchor;\n")
    print("const LE = \"le\"; //little endian\n")

    for src_file in files:
        f = open(join(toplevel, src_file), "r")
        lines = f.readlines()
        f.close()
        parse_structs(lines)

    for i in structs:
        if i in skips:
            continue

        print(f"const {i}Layout = BufferLayout.struct([")
        generate_buflayout(i)
        print(f"]);\n")

        print(f"function decode_{camel_to_snake(i)}(buf) {{")
        print(f"    let raw = {i}Layout.decode(buf);")
        print("    return {")
        generate_decoder(i)
        print("    };\n}\n")

        print(f"interface {i} {{")
        generate_interface(i)
        print("}\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
