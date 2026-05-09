package dev.craftmc.dealsign;

import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerChatEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;

import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;

/**
 * Event listener for player messages (chat, join, leave).
 * Sends these events to the CraftMC manager's mcsapi.
 */
public class MessageListeners implements Listener {

    private final DealSignPlugin plugin;
    private final HttpClient http;
    private final String apiBase;

    public MessageListeners(DealSignPlugin plugin, HttpClient http, String apiBase) {
        this.plugin = plugin;
        this.http = http;
        this.apiBase = apiBase;
    }

    @EventHandler
    public void onPlayerChat(PlayerChatEvent event) {
        if (event.isCancelled()) return;
        Player player = event.getPlayer();
        String message = event.getMessage();
        postMessage(player.getName(), "chat", message);
    }

    @EventHandler
    public void onPlayerJoin(PlayerJoinEvent event) {
        Player player = event.getPlayer();
        postMessage(player.getName(), "join", "");
    }

    @EventHandler
    public void onPlayerQuit(PlayerQuitEvent event) {
        Player player = event.getPlayer();
        postMessage(player.getName(), "leave", "");
    }

    private void postMessage(String playerName, String type, String content) {
        String url = apiBase + "/mcsapi/send_message";
        String body = String.format("{\"name\":\"%s\",\"type\":\"%s\",\"content\":\"%s\"}",
                escapeJson(playerName),
                escapeJson(type),
                escapeJson(content));

        HttpRequest req = HttpRequest.newBuilder(URI.create(url))
                .timeout(Duration.ofSeconds(plugin.getConfig().getInt("timeout-seconds", 10)))
                .header("Content-Type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(body))
                .build();

        http.sendAsync(req, HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8))
                .whenComplete((resp, err) -> {
                    // Fire and forget - log errors if needed
                    if (err != null) {
                        plugin.getLogger().warning("Failed to post message for " + playerName + ": " + err.getMessage());
                    } else if (resp.statusCode() >= 400) {
                        plugin.getLogger().warning("API error posting message: " + resp.statusCode() + " " + resp.body());
                    }
                });
    }

    private static String escapeJson(String s) {
        if (s == null) return "";
        return s.replace("\\", "\\\\")
                .replace("\"", "\\\"")
                .replace("\n", "\\n")
                .replace("\r", "\\r")
                .replace("\t", "\\t");
    }
}
