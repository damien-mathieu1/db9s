// Runs once on first `docker compose up` (empty mongo volume), mirroring dev/seed.sql.
db = db.getSiblingDB("db9s");

db.users.insertMany([
  { name: "Ada Lovelace", email: "ada@example.com", active: true, created_at: new Date() },
  { name: "Alan Turing", email: "alan@example.com", active: true, created_at: new Date() },
  { name: "Grace Hopper", email: "grace@example.com", active: false, created_at: new Date() },
]);

db.products.insertMany([
  { name: "Keyboard", price: 49.9, in_stock: true, tags: ["hardware", "input"], attributes: { layout: "azerty", wireless: true } },
  { name: "Monitor", price: 199.0, in_stock: false, tags: ["hardware", "display"], attributes: { size: 27, resolution: "2560x1440" } },
  { name: "Mouse", price: 19.5, in_stock: true, tags: ["hardware", "input"], attributes: { dpi: 1600 } },
]);
