import type { RuntimeKind } from "@/api/gen/RuntimeKind";

export const runtimeLabel: Record<RuntimeKind, string> = {
  php: "PHP",
  mysql: "MySQL",
  mariadb: "MariaDB",
  postgres: "PostgreSQL",
  valkey: "Valkey",
  node: "Node.js",
  bun: "Bun",
  composer: "Composer",
  cloudflared: "cloudflared",
};
