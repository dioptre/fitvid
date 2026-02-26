#!/bin/bash
set -e

echo "🦀 Building Rust WASM..."

# Build with wasm-pack
wasm-pack build --target web --out-dir pkg

echo "✅ WASM build complete!"
echo ""
echo "📦 Installing web dependencies..."

cd www
npm install

echo "✅ Setup complete!"
echo ""
echo "To start development server:"
echo "  cd www && npm run dev"
echo ""
echo "To build for production:"
echo "  cd www && npm run build"
