package dev.craftmc.dealsign;

import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;
import org.bukkit.plugin.java.JavaPlugin;

import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Locale;

/**
 * Lightweight Paper plugin that wires the in-game /sign /reject /deal /deals
 * commands to the CraftMC manager's public mcsapi endpoints.
 *
 * Endpoints used:
 *   GET /mcsapi/record/list
 *   GET /mcsapi/record/view?dealId=...
 *   GET /mcsapi/record/approve?name=...&dealId=...
 *   GET /mcsapi/record/reject?name=...&dealId=...&reason=...
 */
public final class DealSignPlugin extends JavaPlugin {

    private HttpClient http;
    private String apiBase;

    @Override
    public void onEnable() {
        saveDefaultConfig();
        reloadFromConfig();
        this.http = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(getConfig().getInt("timeout-seconds", 10)))
                .build();
        getLogger().info("DealSign enabled — api-base=" + apiBase);
    }

    private void reloadFromConfig() {
        String base = getConfig().getString("api-base", "http://localhost:3000");
        if (base.endsWith("/")) base = base.substring(0, base.length() - 1);
        this.apiBase = base;
    }

    @Override
    public boolean onCommand(CommandSender sender, Command cmd, String label, String[] args) {
        if (!(sender instanceof Player)) {
            sender.sendMessage("This command is for players.");
            return true;
        }
        Player p = (Player) sender;
        String name = p.getName();

        switch (cmd.getName().toLowerCase(Locale.ROOT)) {
            case "sign":
                if (args.length < 1) { p.sendMessage(ChatColor.RED + "Usage: /sign <dealId>"); return true; }
                callApprove(p, name, args[0]);
                return true;
            case "reject": {
                if (args.length < 1) { p.sendMessage(ChatColor.RED + "Usage: /reject <dealId> [reason...]"); return true; }
                String reason = null;
                if (args.length > 1) {
                    StringBuilder sb = new StringBuilder();
                    for (int i = 1; i < args.length; i++) { if (i > 1) sb.append(' '); sb.append(args[i]); }
                    reason = sb.toString();
                }
                callReject(p, name, args[0], reason);
                return true;
            }
            case "deal":
                if (args.length < 1) { p.sendMessage(ChatColor.RED + "Usage: /deal <dealId>"); return true; }
                callView(p, args[0]);
                return true;
            case "deals":
                callListForPlayer(p, name);
                return true;
        }
        return false;
    }

    // ---------- API calls (run async, deliver result on main thread) ----------

    private void callApprove(Player p, String name, String dealId) {
        String url = apiBase + "/mcsapi/record/approve?name=" + enc(name) + "&dealId=" + enc(dealId);
        getJsonAsync(url, (status, body, err) -> {
            if (err != null) { msg(p, ChatColor.RED + "Network error: " + err); return; }
            if (status >= 200 && status < 300) {
                msg(p, ChatColor.GREEN + "Signed deal " + ChatColor.WHITE + dealId);
            } else {
                msg(p, ChatColor.RED + "Sign failed: " + extractError(body));
            }
        });
    }

    private void callReject(Player p, String name, String dealId, String reason) {
        StringBuilder url = new StringBuilder(apiBase)
                .append("/mcsapi/record/reject?name=").append(enc(name))
                .append("&dealId=").append(enc(dealId));
        if (reason != null && !reason.isEmpty()) url.append("&reason=").append(enc(reason));
        getJsonAsync(url.toString(), (status, body, err) -> {
            if (err != null) { msg(p, ChatColor.RED + "Network error: " + err); return; }
            if (status >= 200 && status < 300) {
                msg(p, ChatColor.YELLOW + "Rejected deal " + ChatColor.WHITE + dealId);
            } else {
                msg(p, ChatColor.RED + "Reject failed: " + extractError(body));
            }
        });
    }

    private void callView(Player p, String dealId) {
        String url = apiBase + "/mcsapi/record/view?dealId=" + enc(dealId);
        getJsonAsync(url, (status, body, err) -> {
            if (err != null) { msg(p, ChatColor.RED + "Network error: " + err); return; }
            if (status >= 200 && status < 300) {
                String title = pluck(body, "\"title\":\"", "\"");
                String stat = pluck(body, "\"status\":\"", "\"");
                String parties = pluck(body, "\"parties\":[", "]");
                msg(p, ChatColor.GOLD + "Deal " + ChatColor.WHITE + dealId
                        + ChatColor.GRAY + " · " + ChatColor.WHITE + (title == null ? "?" : title));
                msg(p, ChatColor.GRAY + "Status: " + ChatColor.WHITE + stat);
                msg(p, ChatColor.GRAY + "Parties: " + ChatColor.WHITE + (parties == null ? "?" : parties.replace("\"","")));
                msg(p, ChatColor.GRAY + "Type " + ChatColor.WHITE + "/sign " + dealId
                        + ChatColor.GRAY + " or " + ChatColor.WHITE + "/reject " + dealId);
            } else {
                msg(p, ChatColor.RED + "View failed: " + extractError(body));
            }
        });
    }

    private void callListForPlayer(Player p, String name) {
        String url = apiBase + "/mcsapi/record/list";
        getJsonAsync(url, (status, body, err) -> {
            if (err != null) { msg(p, ChatColor.RED + "Network error: " + err); return; }
            if (status < 200 || status >= 300) { msg(p, ChatColor.RED + "List failed: " + extractError(body)); return; }
            // Cheap and cheerful: scan the JSON for {"id":"X" ... "title":"T" ... "parties":[...]}.
            // For richer parsing, drop in a JSON library.
            String[] chunks = body.split("\\{\"id\":\"");
            int matched = 0;
            for (int i = 1; i < chunks.length; i++) {
                String c = "{\"id\":\"" + chunks[i];
                String id = pluck(c, "\"id\":\"", "\"");
                String title = pluck(c, "\"title\":\"", "\"");
                String parties = pluck(c, "\"parties\":[", "]");
                String stat = pluck(c, "\"status\":\"", "\"");
                if (id == null || parties == null) continue;
                if (parties.toLowerCase(Locale.ROOT).contains("\"" + name.toLowerCase(Locale.ROOT) + "\"")) {
                    if (!"signed".equals(stat) && !"rejected".equals(stat)) {
                        msg(p, ChatColor.GOLD + id + ChatColor.GRAY + " · " + ChatColor.WHITE + (title == null ? "?" : title)
                                + ChatColor.GRAY + " (" + stat + ")");
                        matched++;
                    }
                }
            }
            if (matched == 0) msg(p, ChatColor.GRAY + "No pending deals for you.");
        });
    }

    // ---------- HTTP helpers ----------

    @FunctionalInterface
    interface JsonCb { void run(int status, String body, String err); }

    private void getJsonAsync(String url, JsonCb cb) {
        HttpRequest req = HttpRequest.newBuilder(URI.create(url))
                .timeout(Duration.ofSeconds(getConfig().getInt("timeout-seconds", 10)))
                .header("Accept", "application/json")
                .GET().build();
        http.sendAsync(req, HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8))
                .whenComplete((resp, err) -> Bukkit.getScheduler().runTask(this, () -> {
                    if (err != null) cb.run(0, null, err.getMessage());
                    else cb.run(resp.statusCode(), resp.body(), null);
                }));
    }

    private static String enc(String s) { return URLEncoder.encode(s, StandardCharsets.UTF_8); }
    private static void msg(Player p, String s) { p.sendMessage("[Deal] " + s); }

    private static String extractError(String body) {
        if (body == null) return "no response";
        String e = pluck(body, "\"error\":\"", "\"");
        return e != null ? e : (body.length() > 120 ? body.substring(0, 120) + "..." : body);
    }

    private static String pluck(String hay, String start, String end) {
        if (hay == null) return null;
        int i = hay.indexOf(start); if (i < 0) return null;
        int from = i + start.length();
        int j = hay.indexOf(end, from); if (j < 0) return null;
        return hay.substring(from, j);
    }
}
