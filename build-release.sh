#!/bin/bash

cd "$(dirname "$0")"

version=$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[0].version')
distdir=./dist/"$version"
binaryname="cdecao"
mkdir -p "$distdir"

echo ">>>>>> Building for Linux on amd64"
cargo build --release --target=x86_64-unknown-linux-gnu
mv target/x86_64-unknown-linux-gnu/release/"$binaryname" "$distdir"/"$binaryname"-linux-x86_64

echo ">>>>>> Building for Linux on modern amd64 (x68-64-v3)"
RUSTFLAGS="-C target-cpu=x86-64-v3" CARGO_TARGET_DIR="target_x86-64-v3" cargo build --release --target=x86_64-unknown-linux-gnu
mv target_x86-64-v3/x86_64-unknown-linux-gnu/release/"$binaryname" "$distdir"/"$binaryname"-linux-x86_64-v3

echo ">>>>>> Building for Windows on amd64"
cargo build --release --target=x86_64-pc-windows-gnu
mv target/x86_64-pc-windows-gnu/release/"$binaryname".exe "$distdir"/"$binaryname"-win32-x86_64.exe

echo ">>>>>> Building for Windowsx on modern amd64 (x68-64-v3)"
RUSTFLAGS="-C target-cpu=x86-64-v3" CARGO_TARGET_DIR="target_x86-64-v3" cargo build --release --target=x86_64-pc-windows-gnu
mv target_x86-64-v3/x86_64-pc-windows-gnu/release/"$binaryname".exe "$distdir"/"$binaryname"-win32-x86_64-v3.exe
