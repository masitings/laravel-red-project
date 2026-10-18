import { useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import type { Event } from "./gen/Event";
import { keys } from "./queries";

/** Refetch whatever a daemon event says has changed. */
export function useDaemonEvents() {
  const client = useQueryClient();
  useEffect(() => {
    if (!isTauri()) return;
    const refresh = (event: Event) => {
      switch (event.type) {
        case "connected":
          client.invalidateQueries();
          break;
        case "sites_changed":
          client.invalidateQueries({ queryKey: keys.sites });
          client.invalidateQueries({ queryKey: keys.doctor });
          break;
        case "proxies_changed":
          client.invalidateQueries({ queryKey: keys.proxies });
          break;
        case "php_changed":
          client.invalidateQueries({ queryKey: keys.php });
          client.invalidateQueries({ queryKey: keys.sites });
          break;
      }
    };
    const unlisten = listen<Event>("red://event", (message) => refresh(message.payload));
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [client]);
}
