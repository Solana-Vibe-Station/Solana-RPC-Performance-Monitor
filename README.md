# Solana RPC Performance Monitor

This Rust project **monitors multiple Solana RPC endpoints**, fetching and storing the response times and block height, while also calculating consensus metrics.





## 🚀 Features
- **Monitors multiple Solana RPCs concurrently** (async, non-blocking)
- **Stores RPC metrics in RocksDB** for historical analysis
- **Calculates RPC response time metrics and visualizes this data on a time chart**
- **Web UI served with Axum** (`/static/index.html`)

---

## 🛠 Installation & Setup

### 1️⃣ **Clone the Repository**

### 2️⃣ **Install Rust (If Not Installed)**
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Verify installation:
```sh
rustc --version
cargo --version
```

### 3️⃣ **Install Dependencies**

```
sudo apt install pkg-config
sudo apt install librust-openssl-dev
```

```sh
cargo build
```

---

## ⚙️ Configuration

### **Edit `config.toml`**
Before running the project, configure the **Solana RPC endpoints** in `config.toml`:
```toml
[rpc]
endpoints = [
    { url = "https://api.mainnet-beta.solana.com", nickname = "Mainnet" },
    { url = "https://api.devnet.solana.com", nickname = "Devnet" },
    { url = "https://solana-api.projectserum.com", nickname = "ProjectSerum" },
    { url = "https://rpc.ankr.com/solana", nickname = "Ankr" }
]
```
- You can **add/remove** endpoints as needed.
- Each endpoint must have a **nickname** for easier identification.

---

## ▶️ Running the Program

Run the program with:
```sh
cargo run
```
It will start monitoring Solana RPCs and provide API access.

Example output:
```
Starting RPC monitoring...
Querying Mainnet: https://api.mainnet-beta.solana.com
Querying Devnet: https://api.devnet.solana.com
Querying ProjectSerum: https://solana-api.projectserum.com
Querying Ankr: https://rpc.ankr.com/solana
[Mainnet] Slot: 202832145, Blockhash: G6sj1rBdL2Kt... (120ms)
...
Server running on http://localhost:3000
```

---

## 📊 Web UI

A **basic web interface** is available at:
```sh
http://localhost:3000/static/index.html
```
If missing, generate the static directory:
```sh
mkdir static
echo '<h1>Solana RPC Monitoring</h1>' > static/index.html
```

---

## 🛠 Troubleshooting

### **❌ `rocksdb: IO error`**
**Fix:** Ensure the database directory is writable and restart the application.

### **❌ `Server not starting`**
**Fix:** Check if port **3000** is free or specify another:
```sh
SERVER_PORT=4000 cargo run
```

---

## 📜 License
This project is licensed under the **MIT License**.

---

## 🤝 Contributing
Feel free to submit issues or pull requests to improve this project!

---

## 🔗 Resources
- [Solana Documentation](https://docs.solana.com/)
- [Rust Async Book](https://rust-lang.github.io/async-book/)
- [Axum Documentation](https://docs.rs/axum/latest/axum/)
- [RocksDB Rust Docs](https://docs.rs/rocksdb/latest/rocksdb/)
```

