import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vite-plus/test";
import { httpServer } from "../../../../tools/web/test/setup";
import { adminHandlers } from "../../../../tools/web/mocks/admin";
import { captureTraces } from "../../../../tools/web/test/traces";
import type { AdminSession, Garage, Grant, PlayerRecord } from "../../application/admin-contracts";
import type { StoreOffer } from "../../../player_sessions/application/store-contracts";
import Admin from "./admin-page";

const origin = "http://localhost:8080/api/admin";
const password = "private-admin-password";
const admin: PlayerRecord = {
  id: "1",
  username: "admin@example.test",
  display_name: "Administrator One",
  role: "admin",
};
const player: PlayerRecord = {
  id: "123",
  username: "private-login@example.test",
  display_name: "Commander",
  role: "player",
};
const authenticated: AdminSession = {
  status: "authenticated",
  player: admin,
  store_enabled: false,
};
const garage: Garage = {
  display_name: player.display_name,
  credits: "100",
  gold: "0",
  bonds: "5",
  free_xp: "0",
  slots: 3,
  attachments: { 12: 1 },
  vehicles: [
    {
      key: "usa:A01_T1_Cunningham",
      name: "T1 Cunningham",
      inventory_id: 209,
      tier: 1,
      stat_trackers: { 37: 0 },
    },
  ],
};

function openAdmin(signedIn = false) {
  window.history.replaceState(null, "", "/dashboard/");
  if (signedIn)
    httpServer.use(
      http.get(`${origin}/session`, () => HttpResponse.json(authenticated)),
      http.get(`${origin}/me/garage`, () => HttpResponse.json(garage)),
    );
  return { user: userEvent.setup(), ...render(<Admin />) };
}

function players(records = [player]) {
  return http.get(`${origin}/players`, () =>
    HttpResponse.json({ data: records, total: records.length }),
  );
}

const passwordInput = () => screen.getByLabelText("Password", { selector: "input", exact: false });

describe("Server Administration", () => {
  it("gives the demo administrator player tabs and keeps sign-out across a reload", async () => {
    localStorage.clear();
    httpServer.use(...adminHandlers());
    try {
      const first = openAdmin();
      const tabs = await screen.findByRole("tablist", { name: "Player dashboard" });
      expect(within(tabs).getByRole("tab", { name: "My garage" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
      await first.user.click(within(tabs).getByRole("tab", { name: "Store" }));
      expect(await screen.findByRole("heading", { name: "WoT Premium Account" })).toBeVisible();
      expect(screen.getByRole("tablist", { name: "Store sections" })).toBeVisible();
      await first.user.click(within(tabs).getByRole("tab", { name: "Admin" }));
      expect(await screen.findByRole("heading", { name: "Player accounts" })).toBeVisible();
      expect(within(tabs).getByRole("tab", { name: "Store" })).toBeVisible();
      await first.user.click(screen.getByRole("button", { name: "Sign out" }));
      expect(await screen.findByRole("button", { name: "Sign in" })).toBeVisible();

      first.unmount();
      httpServer.resetHandlers();
      httpServer.use(...adminHandlers());
      openAdmin();
      expect(await screen.findByRole("button", { name: "Sign in" })).toBeVisible();
      expect(screen.queryByRole("tablist", { name: "Player dashboard" })).not.toBeInTheDocument();
    } finally {
      localStorage.clear();
    }
  });

  it("buys from the dashboard Store and refreshes the player's garage balances", async () => {
    const telemetry = captureTraces();
    const offer: StoreOffer = {
      item: -1,
      name: "1 day",
      category: "premium_account",
      price: { currency: "gold", amount: 250 },
      in_depot: 0,
      availability: "available",
    };
    let revision = "9";
    let gold = "500";
    let catalogTrace: string | null = null;
    let orderTrace: string | null = null;
    let order: unknown;
    httpServer.use(
      http.get(`${origin}/session`, () =>
        HttpResponse.json({ status: "authenticated", player, store_enabled: true }),
      ),
      http.get(`${origin}/me/garage`, () => HttpResponse.json({ ...garage, gold })),
      http.get(`${origin}/me/shop/catalog`, ({ request }) => {
        expect(request.headers.get("authorization")).toBeNull();
        catalogTrace = request.headers.get("traceparent");
        return HttpResponse.json({ revision, premium_expires_at: null, offers: [offer] });
      }),
      http.post(`${origin}/me/shop/orders`, async ({ request }) => {
        expect(request.headers.get("authorization")).toBeNull();
        expect(request.headers.get("x-wot-admin")).toBe("1");
        orderTrace = request.headers.get("traceparent");
        order = await request.json();
        revision = "10";
        gold = "250";
        return HttpResponse.json({
          revision,
          premium_expires_at: Math.floor(Date.now() / 1000) + 86400,
          offers: [offer],
        });
      }),
    );
    try {
      const { user } = openAdmin();
      expect(await screen.findByText("500")).toBeVisible();
      await user.click(screen.getByRole("tab", { name: "Store" }));
      await user.click(await screen.findByRole("button", { name: "Buy 1 day for 250 gold" }));
      expect(await screen.findByText("Purchased 1 day.")).toBeVisible();
      expect(screen.getByText(/active until/i)).toBeVisible();
      expect(order).toEqual({ item: offer.item, revision: "9", price: offer.price });
      await user.click(screen.getByRole("tab", { name: "My garage" }));
      expect(await screen.findByText("250")).toBeVisible();
      telemetry.expectRequest(
        catalogTrace,
        "web.store.load_catalog",
        "/api/admin/me/shop/catalog",
        200,
      );
      const { parent } = telemetry.expectRequest(
        orderTrace,
        "web.store.purchase",
        "/api/admin/me/shop/orders",
        200,
      );
      expect(parent?.attributes).toMatchObject({
        "input.item": -1,
        "input.revision": "9",
        "input.price.currency": "gold",
        "input.price.amount": 250,
        "output.offer_count": 1,
      });
    } finally {
      await telemetry.stop();
    }
  });

  it("returns to sign-in when the dashboard session expires while opening the Store", async () => {
    const telemetry = captureTraces();
    let session: AdminSession = { status: "authenticated", player, store_enabled: true };
    let catalogTrace: string | null = null;
    httpServer.use(
      http.get(`${origin}/session`, () => HttpResponse.json(session)),
      http.get(`${origin}/me/garage`, () => HttpResponse.json(garage)),
      http.get(`${origin}/me/shop/catalog`, ({ request }) => {
        catalogTrace = request.headers.get("traceparent");
        session = { status: "signed_out", registration_open: true };
        return HttpResponse.text("Sign in to open the Store.", { status: 401 });
      }),
    );
    try {
      const { user } = openAdmin();
      await user.click(await screen.findByRole("tab", { name: "Store" }));
      expect(await screen.findByRole("button", { name: "Sign in" })).toBeVisible();
      const denied = telemetry.expectRequest(
        catalogTrace,
        "web.store.load_catalog",
        "/api/admin/me/shop/catalog",
        401,
      );
      expect(denied.http?.attributes["output.outcome"]).toBe("rejected");
    } finally {
      await telemetry.stop();
    }
  });

  it("rejects incorrect credentials, then signs in and signs out", async () => {
    const telemetry = captureTraces();
    const attempts: Array<{ body: unknown; trace: string | null }> = [];
    let session: AdminSession = { status: "signed_out", registration_open: true };
    let signOutTrace: string | null = null;
    httpServer.use(
      http.get(`${origin}/me/garage`, () => HttpResponse.json(garage)),
      http.get(`${origin}/session`, () => HttpResponse.json(session)),
      http.post(`${origin}/logout`, ({ request }) => {
        signOutTrace = request.headers.get("traceparent");
        expect(request.headers.get("x-wot-admin")).toBe("1");
        session = { status: "signed_out", registration_open: true };
        return new HttpResponse(null, { status: 204 });
      }),
      http.post(`${origin}/session`, async ({ request }) => {
        const body = (await request.json()) as { email: string; password: string };
        attempts.push({ body, trace: request.headers.get("traceparent") });
        expect(request.headers.get("x-wot-admin")).toBe("1");
        if (body.email === admin.username && body.password === password) {
          session = authenticated;
          return new HttpResponse(null, { status: 204 });
        }
        return HttpResponse.json({ message: "Invalid email or password." }, { status: 401 });
      }),
    );
    try {
      const { user } = openAdmin();
      await user.type(await screen.findByRole("textbox", { name: "Email" }), admin.username);
      await user.type(passwordInput(), "bad-password");
      await user.click(screen.getByRole("button", { name: "Sign in" }));
      expect(await screen.findByText("Invalid email or password.")).toBeVisible();
      await user.clear(passwordInput());
      await user.type(passwordInput(), password);
      await user.click(screen.getByRole("button", { name: "Sign in" }));
      expect(await screen.findByText(admin.display_name)).toBeVisible();
      expect(attempts.map((item) => item.body)).toEqual([
        { email: admin.username, password: "bad-password" },
        { email: admin.username, password },
      ]);
      const denied = telemetry.expectRequest(
        attempts[0].trace,
        "web.admin.sign_in",
        "/api/admin/session",
        401,
      );
      expect(denied.http?.attributes["output.outcome"]).toBe("rejected");
      telemetry.expectRequest(attempts[1].trace, "web.admin.sign_in", "/api/admin/session", 204);
      await user.click(screen.getByRole("button", { name: "Sign out" }));
      expect(await screen.findByRole("button", { name: "Sign in" })).toBeVisible();
      telemetry.expectRequest(signOutTrace, "web.admin.sign_out", "/api/admin/logout", 204);
      expect(telemetry.serializedSpanData()).not.toContain(password);
      expect(telemetry.serializedSpanData()).not.toContain(admin.username);
    } finally {
      await telemetry.stop();
    }
  });

  it("creates the first administrator and requires a separate sign-in", async () => {
    const telemetry = captureTraces();
    let session: AdminSession = { status: "setup_required" };
    let setup: unknown;
    let trace: string | null = null;
    httpServer.use(
      http.get(`${origin}/session`, () => HttpResponse.json(session)),
      http.post(`${origin}/setup`, async ({ request }) => {
        setup = await request.json();
        trace = request.headers.get("traceparent");
        session = { status: "signed_out", registration_open: true };
        return HttpResponse.json(admin, { status: 201 });
      }),
    );
    try {
      const { user } = openAdmin();
      expect(
        await screen.findByRole("heading", { name: "Create the first administrator" }),
      ).toBeVisible();
      expect(
        screen.queryByRole("button", { name: "Create a player account" }),
      ).not.toBeInTheDocument();
      await user.type(screen.getByRole("textbox", { name: "Email" }), admin.username);
      await user.type(screen.getByRole("textbox", { name: "Display name" }), admin.display_name);
      await user.type(passwordInput(), password);
      await user.click(screen.getByRole("button", { name: "Create administrator" }));
      expect(await screen.findByText("Administrator created. Sign in to continue.")).toBeVisible();
      expect(screen.getByRole("button", { name: "Sign in" })).toBeVisible();
      expect(passwordInput()).toHaveValue("");
      expect(screen.getByRole("button", { name: "Create a player account" })).toBeVisible();
      expect(setup).toEqual({
        username: admin.username,
        display_name: admin.display_name,
        password,
      });
      telemetry.expectRequest(trace, "web.admin.bootstrap", "/api/admin/setup", 201);
      expect(telemetry.serializedSpanData()).not.toContain(password);
      expect(telemetry.serializedSpanData()).not.toContain(admin.username);
    } finally {
      await telemetry.stop();
    }
  });

  it("registers a player and opens their read-only garage after sign-in", async () => {
    const telemetry = captureTraces();
    let session: AdminSession = { status: "signed_out", registration_open: true };
    const traces = new Map<string, string | null>();
    const passphrase = "Tank123!";
    const takenEmail = "taken@example.test";
    const emailConflict = "That email is already in use.";
    const registered = { ...player, display_name: "NewCommander" };
    httpServer.use(
      http.get(`${origin}/session`, () => HttpResponse.json(session)),
      http.post(`${origin}/registration/username`, async ({ request }) => {
        const { username } = (await request.json()) as { username: string };
        expect(request.headers.get("x-wot-admin")).toBe("1");
        traces.set(username, request.headers.get("traceparent"));
        return HttpResponse.json({ available: username !== "TakenName" });
      }),
      http.post(`${origin}/registration`, async ({ request }) => {
        expect(request.headers.get("x-wot-admin")).toBe("1");
        const body = (await request.json()) as { email?: unknown } | null;
        if (body?.email === takenEmail) {
          traces.set("duplicate", request.headers.get("traceparent"));
          return HttpResponse.json(
            { message: emailConflict, errors: { email: emailConflict } },
            { status: 409 },
          );
        }
        expect(body).toEqual({
          email: registered.username,
          username: registered.display_name,
          password: passphrase,
          confirm_password: passphrase,
        });
        traces.set("register", request.headers.get("traceparent"));
        return HttpResponse.json(registered, { status: 201 });
      }),
      http.post(`${origin}/session`, async ({ request }) => {
        expect(await request.json()).toEqual({ email: registered.username, password: passphrase });
        session = { status: "authenticated", player: registered, store_enabled: false };
        return new HttpResponse(null, { status: 204 });
      }),
      http.get(`${origin}/me/garage`, ({ request }) => {
        traces.set("garage", request.headers.get("traceparent"));
        return HttpResponse.json({ ...garage, display_name: registered.display_name });
      }),
      http.post(`${origin}/logout`, () => {
        session = { status: "signed_out", registration_open: true };
        return new HttpResponse(null, { status: 204 });
      }),
    );

    try {
      const { user } = openAdmin();
      await user.click(await screen.findByRole("button", { name: "Create a player account" }));
      const username = screen.getByRole("textbox", { name: "Username" });
      await user.type(username, "TakenName");
      expect(await screen.findByText("That username is already taken.")).toBeVisible();
      expect(screen.getByRole("button", { name: "Create account" })).toBeDisabled();
      await user.clear(username);
      await user.type(username, registered.display_name);
      expect(await screen.findByText("Username available.")).toBeVisible();
      const email = screen.getByRole("textbox", { name: "Email" });
      const password = screen.getByLabelText(/^Password/);
      const confirmation = screen.getByLabelText(/^Confirm password/);
      const submit = screen.getByRole("button", { name: "Create account" });
      await user.type(email, "new");
      await user.click(submit);
      expect(email).toHaveFocus();
      expect(email).toHaveAccessibleDescription(
        "Enter a valid email address of up to 128 characters.",
      );
      await user.clear(email);
      await user.type(email, "new@example");
      await user.tab();
      expect(email).toBeInvalid();
      expect(email).toHaveAccessibleDescription(
        "Enter a valid email address of up to 128 characters.",
      );
      await user.clear(email);
      await user.type(email, takenEmail);
      expect(email).toBeValid();

      for (const weakPassword of ["Tank1!", "TankTest!", "tank123!", "TANK123!", "Tank123 "]) {
        await user.clear(password);
        await user.type(password, weakPassword);
        await user.clear(confirmation);
        await user.type(confirmation, weakPassword);
        await user.click(submit);
        expect(password).toBeInvalid();
        expect(password).toHaveAccessibleDescription(/at least one digit/);
      }
      expect(traces.has("duplicate")).toBe(false);
      await user.clear(password);
      await user.type(password, passphrase);
      await user.clear(confirmation);
      await user.type(confirmation, "different passphrase");
      await user.click(submit);
      expect(await screen.findByText("The passwords do not match.")).toBeVisible();
      await user.clear(confirmation);
      await user.type(confirmation, passphrase);
      await user.click(submit);
      expect(await screen.findByText(emailConflict)).toBeVisible();
      expect(email).toBeInvalid();
      expect(email).toHaveAccessibleDescription(emailConflict);
      await user.clear(email);
      await user.type(email, registered.username);
      expect(email).toBeValid();
      await user.click(submit);
      expect(await screen.findByRole("heading", { name: "Account created" })).toBeVisible();
      expect(screen.getByText("How to sign in to World of Tanks")).toBeVisible();
      expect(screen.getByText(registered.username)).toBeVisible();

      await user.click(screen.getByRole("button", { name: "Sign in to the panel" }));
      await user.type(screen.getByRole("textbox", { name: "Email" }), registered.username);
      await user.type(passwordInput(), passphrase);
      await user.click(screen.getByRole("button", { name: "Sign in" }));
      expect(await screen.findByRole("heading", { name: "My garage" })).toBeVisible();
      expect(await screen.findByRole("heading", { name: "Current balances" })).toBeVisible();
      expect(screen.getByText("Bonds")).toBeVisible();
      expect(screen.getByRole("rowheader", { name: "T1 Cunningham" })).toBeVisible();
      expect(screen.queryByText("Player accounts")).not.toBeInTheDocument();
      expect(screen.queryByRole("tab", { name: "Store" })).not.toBeInTheDocument();
      expect(screen.queryByRole("tab", { name: "Admin" })).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Grant/ })).not.toBeInTheDocument();

      const unavailable = telemetry.expectRequest(
        traces.get("TakenName") ?? null,
        "web.panel.check_username",
        "/api/admin/registration/username",
        200,
      );
      expect(unavailable.parent?.attributes["output.username.available"]).toBe(false);
      const duplicate = telemetry.expectRequest(
        traces.get("duplicate") ?? null,
        "web.panel.register",
        "/api/admin/registration",
        409,
      );
      expect(duplicate.parent?.attributes["output.outcome"]).toBe("conflict");
      expect(duplicate.parent?.attributes["input.email.bytes"]).toBe(takenEmail.length);
      telemetry.expectRequest(
        traces.get("register") ?? null,
        "web.panel.register",
        "/api/admin/registration",
        201,
      );
      telemetry.expectRequest(
        traces.get("garage") ?? null,
        "web.panel.load_own_garage",
        "/api/admin/me/garage",
        200,
      );
      for (const input of [
        registered.username,
        registered.display_name,
        "TakenName",
        takenEmail,
        passphrase,
      ]) {
        expect(telemetry.serializedSpanData()).not.toContain(input);
      }
      await user.click(screen.getByRole("button", { name: "Sign out" }));
      expect(await screen.findByRole("button", { name: "Sign in" })).toBeVisible();
    } finally {
      await telemetry.stop();
    }
  });

  it("promotes and demotes another player without exposing a self-role control", async () => {
    const telemetry = captureTraces();
    const records = [admin, { ...player }];
    const changes: Array<{ body: unknown; trace: string | null }> = [];
    httpServer.use(
      players(records),
      http.post(`${origin}/players/${player.id}/role`, async ({ request }) => {
        const body = (await request.json()) as { role: PlayerRecord["role"] };
        changes.push({ body, trace: request.headers.get("traceparent") });
        records[1].role = body.role;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    try {
      const { user } = openAdmin(true);
      await user.click(await screen.findByRole("tab", { name: "Admin" }));
      await user.click(
        await screen.findByRole("button", { name: "Promote to administrator · Commander" }),
      );
      expect(
        screen.queryByRole("button", { name: /demote.*administrator one/i }),
      ).not.toBeInTheDocument();
      await user.click(
        within(screen.getByRole("dialog")).getByRole("button", {
          name: "Promote to administrator",
        }),
      );
      await user.click(await screen.findByRole("button", { name: "Demote to player · Commander" }));
      await user.click(
        within(screen.getByRole("dialog")).getByRole("button", { name: "Demote to player" }),
      );
      expect(
        await screen.findByRole("button", { name: "Promote to administrator · Commander" }),
      ).toBeVisible();
      expect(changes.map(({ body }) => body)).toEqual([{ role: "admin" }, { role: "player" }]);
      for (const { trace } of changes) {
        telemetry.expectRequest(
          trace,
          "web.admin.change_role",
          "/api/admin/players/{id}/role",
          204,
        );
      }
    } finally {
      await telemetry.stop();
    }
  });

  it("creates a player through the form without exporting login or password fields", async () => {
    const telemetry = captureTraces();
    const records = [player];
    const newPlayer = {
      username: "recruit@example.test",
      display_name: "Recruit",
      password: "secret-new-account",
    };
    let sent: unknown;
    let traceHeader: string | null = null;
    httpServer.use(
      players(records),
      http.post(`${origin}/players`, async ({ request }) => {
        sent = await request.json();
        traceHeader = request.headers.get("traceparent");
        const record = {
          id: "124",
          display_name: newPlayer.display_name,
          username: newPlayer.username,
          role: "player" as const,
        };
        records.push(record);
        return HttpResponse.json(record, { status: 201 });
      }),
    );
    try {
      const { user } = openAdmin(true);
      await user.click(await screen.findByRole("tab", { name: "Admin" }));
      await user.click(await screen.findByRole("link", { name: "Create" }));
      await user.type(await screen.findByRole("textbox", { name: /login/i }), newPlayer.username);
      await user.type(
        screen.getByRole("textbox", { name: /display name/i }),
        newPlayer.display_name,
      );
      await user.type(
        screen.getByLabelText(/password/i, { selector: "input" }),
        newPlayer.password,
      );
      await user.click(screen.getByRole("button", { name: "Create player" }));
      expect(await screen.findByText(newPlayer.display_name)).toBeVisible();
      expect(sent).toEqual(newPlayer);
      telemetry.expectRequest(traceHeader, "web.admin.create_player", "/api/admin/players", 201);
      expect(telemetry.serializedSpanData()).not.toContain(newPlayer.password);
      expect(telemetry.serializedSpanData()).not.toContain(newPlayer.username);
    } finally {
      await telemetry.stop();
    }
  });

  it("uses named garage pickers and submits resource, vehicle and cosmetic grants", async () => {
    const telemetry = captureTraces();
    const grants: Array<{ body: Grant; trace: string | null }> = [];
    let snapshot = structuredClone(garage);
    const tank = { id: 813, key: "germany:G01_Pz_IV", name: "Panzer IV", tier: 5 };
    httpServer.use(
      players(),
      http.get(`${origin}/players/123/garage`, () => HttpResponse.json(snapshot)),
      http.get(`${origin}/vehicles`, () =>
        HttpResponse.json([{ id: 101, ...garage.vehicles[0] }, tank]),
      ),
      http.get(`${origin}/attachments`, () =>
        HttpResponse.json([
          { id: 12, name: "Field crate", max_owned: 1 },
          { id: 13, name: "Tool box", max_owned: 3 },
        ]),
      ),
      http.get(`${origin}/stat-trackers`, () =>
        HttpResponse.json([
          { id: 37, name: "Victories", max_owned: 1 },
          { id: 38, name: "Unrelated counter", max_owned: 1 },
        ]),
      ),
      http.post(`${origin}/players/123/grants`, async ({ request }) => {
        const body = (await request.json()) as Grant;
        grants.push({ body, trace: request.headers.get("traceparent") });
        if (grants.length === 1)
          return HttpResponse.json({ message: "Grant temporarily unavailable." }, { status: 503 });
        if (body.kind === "currency") snapshot = { ...snapshot, credits: "150" };
        if (body.kind === "vehicle")
          snapshot.vehicles.push({ ...tank, inventory_id: 210, stat_trackers: {} });
        if (body.kind === "attachment") snapshot.attachments[body.id] = body.count;
        if (body.kind === "stat_tracker") snapshot.vehicles[0].stat_trackers[body.id] = body.count;
        return HttpResponse.json({ garage: snapshot, delivery: "queued_for_garage" });
      }),
    );
    try {
      const { user } = openAdmin(true);
      await user.click(await screen.findByRole("tab", { name: "Admin" }));
      await user.click(await screen.findByText(player.display_name));
      expect(await screen.findByRole("heading", { name: "Current balances" })).toBeVisible();
      await user.type(screen.getByRole("textbox", { name: /amount to add/i }), "50");
      await user.click(screen.getByRole("button", { name: "Grant resource" }));
      expect(await screen.findByText("Grant temporarily unavailable.")).toBeVisible();
      await user.click(screen.getByRole("button", { name: "Grant resource" }));
      expect(await screen.findByText("150")).toBeVisible();

      await user.click(screen.getByRole("combobox", { name: "Tank" }));
      expect(await screen.findByRole("option", { name: /T1 Cunningham.*Owned/i })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      await user.click(screen.getByRole("option", { name: /Panzer IV/i }));
      await user.click(screen.getByRole("button", { name: "Grant tank" }));
      expect(await screen.findByRole("rowheader", { name: "Panzer IV" })).toBeVisible();

      await user.click(screen.getByRole("combobox", { name: "3D attachment" }));
      expect(await screen.findByRole("option", { name: /Field crate/i })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      await user.click(screen.getByRole("option", { name: /Tool box/i }));
      await user.click(screen.getByRole("button", { name: "Grant attachment" }));

      const compatible = screen.getByRole("combobox", { name: "Tank for stat tracker" });
      await user.click(compatible);
      await user.click(await screen.findByRole("option", { name: "T1 Cunningham" }));
      await user.click(screen.getByRole("combobox", { name: "Stat tracker" }));
      expect(await screen.findByRole("option", { name: /Victories/i })).toBeVisible();
      expect(screen.queryByRole("option", { name: /Unrelated counter/i })).toBeNull();
      await user.click(screen.getByRole("option", { name: /Victories/i }));
      await user.click(screen.getByRole("button", { name: "Grant stat tracker" }));
      expect(
        await screen.findByText("Grant applied. A live garage update was queued."),
      ).toBeVisible();

      expect(grants.map((item) => item.body)).toEqual([
        { kind: "currency", currency: "credits", amount: 50 },
        { kind: "currency", currency: "credits", amount: 50 },
        { kind: "vehicle", vehicle: 813 },
        { kind: "attachment", id: 13, count: 1 },
        { kind: "stat_tracker", vehicle: 209, id: 37, count: 1 },
      ]);
      const failure = telemetry.expectRequest(
        grants[0].trace,
        "web.admin.grant",
        "/api/admin/players/{id}/grants",
        503,
      );
      expect(failure.http?.events).toEqual(
        expect.arrayContaining([expect.objectContaining({ name: "exception" })]),
      );
      const success = telemetry.expectRequest(
        grants[4].trace,
        "web.admin.grant",
        "/api/admin/players/{id}/grants",
        200,
      );
      expect(success.parent?.attributes).toMatchObject({
        "input.grant.kind": "stat_tracker",
        "output.delivery": "queued_for_garage",
      });
    } finally {
      await telemetry.stop();
    }
  });
});
