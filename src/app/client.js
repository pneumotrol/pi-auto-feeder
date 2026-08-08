(() => {
    const initializeClocks = (root = document) => {
        const now = performance.now().toString();
        root.querySelectorAll("[data-server-time], [data-cooldown]").forEach(
            (element) => {
                element.dataset.loadedAt = now;
            },
        );
    };

    const tick = () => {
        document.querySelectorAll("[data-server-time]").forEach((element) => {
            const [date, time] = element.dataset.serverTime.split(" ");
            const parts = `${date}-${time}`.split(/[-:]/).map(Number);
            if (parts.length !== 6 || parts.some(Number.isNaN)) return;
            const base = Date.UTC(
                parts[0],
                parts[1] - 1,
                parts[2],
                parts[3],
                parts[4],
                parts[5],
            );
            const elapsed = Math.max(
                0,
                performance.now() - Number(element.dataset.loadedAt || 0),
            );
            const current = new Date(base + elapsed);
            const pad = (value) => String(value).padStart(2, "0");
            element.textContent = `${current.getUTCFullYear()}-${pad(current.getUTCMonth() + 1)}-${pad(current.getUTCDate())} ${pad(current.getUTCHours())}:${pad(current.getUTCMinutes())}:${pad(current.getUTCSeconds())}`;
        });

        document.querySelectorAll("[data-cooldown]").forEach((element) => {
            const elapsed = Math.floor(
                Math.max(
                    0,
                    performance.now() - Number(element.dataset.loadedAt || 0),
                ) / 1000,
            );
            const remaining = Math.max(
                0,
                Number(element.dataset.cooldown) - elapsed,
            );
            const value = element.querySelector("[data-cooldown-value]");
            if (value) value.textContent = String(remaining);
            if (remaining === 0) element.hidden = true;
        });
    };

    document.addEventListener("DOMContentLoaded", () => {
        initializeClocks();
        tick();
        window.setInterval(tick, 1000);

        const events = new EventSource("/events");
        events.onmessage = () => {
            if (window.htmx && document.querySelector('[data-page="home"]')) {
                window.htmx.ajax("GET", "/", {
                    target: "#app",
                    swap: "innerHTML",
                });
            }
        };
    });

    document.addEventListener("htmx:afterSwap", (event) =>
        initializeClocks(event.target),
    );
})();
