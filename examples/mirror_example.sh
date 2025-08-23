#!/bin/bash

# Example script for using the website-mirror utility

echo "🚀 Website Mirror Examples"
echo "=========================="

# Build the project first
echo "📦 Building the project..."
cargo build --release

if [ $? -ne 0 ]; then
    echo "❌ Build failed!"
    exit 1
fi

echo "✅ Build successful!"

# Example 1: Basic mirroring
echo ""
echo "📋 Example 1: Basic website mirroring"
echo "Mirroring example.com to ./basic_mirror"
./target/release/website-mirror https://example.com -o ./basic_mirror

# Example 2: Mirror with depth and concurrency limits
echo ""
echo "📋 Example 2: Advanced mirroring with limits"
echo "Mirroring with depth 5 and 20 concurrent downloads"
./target/release/website-mirror https://httpbin.org \
    --output-dir ./advanced_mirror \
    --max-depth 5 \
    --max-concurrent 20

# Example 3: Mirror with external resources
echo ""
echo "📋 Example 3: Mirror with external resources"
echo "Downloading CSS, JS, and images from external domains"
./target/release/website-mirror https://httpbin.org \
    --output-dir ./external_mirror \
    --download-external \
    --max-depth 3

# Example 4: High-performance mirroring
echo ""
echo "📋 Example 4: High-performance mirroring"
echo "Using 50 concurrent downloads and ignoring robots.txt"
./target/release/website-mirror https://httpbin.org \
    --output-dir ./performance_mirror \
    --max-concurrent 50 \
    --ignore-robots \
    --timeout 60

echo ""
echo "🎉 All examples completed!"
echo "Check the output directories for mirrored websites:"
echo "  - ./basic_mirror"
echo "  - ./advanced_mirror"
echo "  - ./external_mirror"
echo "  - ./performance_mirror" 