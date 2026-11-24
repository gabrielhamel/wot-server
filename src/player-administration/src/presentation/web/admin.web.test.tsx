import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vite-plus/test";
import { httpServer } from "../../../../server-runtime/src/infrastructure/web/test/setup";
import { adminHandlers } from "../../../../server-runtime/src/infrastructure/web/mocks/admin";
import { captureTraces } from "../../../../server-runtime/src/infrastructure/web/test/traces";
import type { AdminSession, Garage, Grant, PlayerRecord } from "../../application/admin-contracts";
import type { StoreOffer } from "../../../../player-sessions/src/web";
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
  equipment_components: "40",
  slots: 3,
  attachments: { 12: 1 },
  styles_2d: {},
  styles_3d: {},
  crew_skins: {},
  special_recruits: {},
  depot: { 286: 2 },
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
      const { http: httpCatalog, parent: catalogOperation } = telemetry.expectRequest(
        catalogTrace,
        "web.store.load_catalog",
        "/api/admin/me/shop/catalog",
        200,
      );
      const { http: httpOrder, parent } = telemetry.expectRequest(
        orderTrace,
        "web.store.purchase",
        "/api/admin/me/shop/orders",
        200,
      );
      expect(httpCatalog?.attributes["ddd.bounded_context.name"]).toBe("player_administration");
      expect(httpOrder?.attributes["ddd.bounded_context.name"]).toBe("player_administration");
      expect(catalogOperation?.attributes["ddd.bounded_context.name"]).toBe("player_sessions");
      expect(parent?.attributes).toMatchObject({
        "ddd.bounded_context.name": "player_sessions",
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
      expect(denied.http?.attributes["ddd.bounded_context.name"]).toBe("player_administration");
      expect(denied.parent?.attributes["ddd.bounded_context.name"]).toBe("player_sessions");
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

  it("validates first administrator credentials and requires a separate sign-in", async () => {
    const telemetry = captureTraces();
    let session: AdminSession = { status: "setup_required" };
    const setupPassword = "Private-admin-password1!";
    const attempts: Array<{ body: unknown; trace: string | null }> = [];
    httpServer.use(
      http.get(`${origin}/session`, () => HttpResponse.json(session)),
      http.post(`${origin}/setup`, async ({ request }) => {
        attempts.push({ body: await request.json(), trace: request.headers.get("traceparent") });
        if (attempts.length === 1) {
          const message = "Enter a valid email address of up to 128 characters.";
          return HttpResponse.json({ message, errors: { username: message } }, { status: 422 });
        }
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
      const email = screen.getByRole("textbox", { name: "Email" });
      const passwordField = passwordInput();
      const create = screen.getByRole("button", { name: "Create administrator" });
      expect(passwordField).toHaveAccessibleDescription(/Use 8 to 128 characters/);
      await user.type(email, "admin@localhost");
      await user.tab();
      expect(email).toBeInvalid();
      expect(email).toHaveAccessibleDescription(
        "Enter a valid email address of up to 128 characters.",
      );
      await user.clear(email);
      await user.type(email, admin.username);
      expect(email).toBeValid();
      await user.type(screen.getByRole("textbox", { name: "Display name" }), admin.display_name);
      await user.type(passwordField, password);
      await user.click(create);
      expect(passwordField).toBeInvalid();
      expect(attempts).toHaveLength(0);
      await user.clear(passwordField);
      await user.type(passwordField, setupPassword);
      await user.click(create);
      expect(
        await screen.findByText("Enter a valid email address of up to 128 characters."),
      ).toBeVisible();
      expect(email).toBeInvalid();
      await user.clear(email);
      await user.type(email, admin.username);
      await user.click(create);
      expect(await screen.findByText("Administrator created. Sign in to continue.")).toBeVisible();
      expect(screen.getByRole("button", { name: "Sign in" })).toBeVisible();
      expect(passwordInput()).toHaveValue("");
      expect(screen.getByRole("button", { name: "Create a player account" })).toBeVisible();
      const expected = {
        username: admin.username,
        display_name: admin.display_name,
        password: setupPassword,
      };
      expect(attempts.map(({ body }) => body)).toEqual([expected, expected]);
      for (const [index, status] of [422, 201].entries()) {
        const { parent } = telemetry.expectRequest(
          attempts[index].trace,
          "web.admin.bootstrap",
          "/api/admin/setup",
          status,
        );
        expect(parent?.attributes).toMatchObject({
          "input.role": "admin",
          "input.email.bytes": admin.username.length,
          "input.display_name.bytes": admin.display_name.length,
          "output.outcome": status === 201 ? "success" : "rejected",
        });
      }
      expect(telemetry.serializedSpanData()).not.toContain(setupPassword);
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
    const registered = { ...player, display_name: "New_Commander9" };
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
      for (const invalidName of ["New Cadet", "New-Cadet", "Éclair"]) {
        await user.type(username, invalidName);
        expect(username).toBeInvalid();
        expect(username).toHaveAccessibleDescription(
          "Use only English letters (A–Z), digits, and underscores.",
        );
        await user.clear(username);
      }
      await user.type(username, "ab");
      await user.tab();
      expect(username).toHaveAccessibleDescription("Use 3 to 24 characters.");
      await user.clear(username);
      await user.paste("a".repeat(25));
      expect(username).toHaveValue("a".repeat(25));
      expect(username).toBeInvalid();
      expect(username).toHaveAccessibleDescription("Use 3 to 24 characters.");
      await user.clear(username);
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

  it("creates a player with registration rules and a generated password", async () => {
    const telemetry = captureTraces();
    const records = [player];
    const email = "recruit@example.test";
    const username = "Recruit_7";
    const duplicateEmail = "taken@example.test";
    const conflict = "That login is already in use.";
    const requests: Array<{
      body: { username: string; display_name: string; password: string };
      trace: string | null;
    }> = [];
    httpServer.use(
      players(records),
      http.post(`${origin}/registration/username`, async ({ request }) => {
        const body = (await request.json()) as { username: string };
        return HttpResponse.json({ available: body.username !== "TakenName" });
      }),
      http.post(`${origin}/players`, async ({ request }) => {
        const body = (await request.json()) as (typeof requests)[number]["body"];
        expect(request.headers.get("x-wot-admin")).toBe("1");
        requests.push({ body, trace: request.headers.get("traceparent") });
        if (body.username === duplicateEmail) {
          return HttpResponse.json(
            { message: conflict, errors: { username: conflict } },
            { status: 409 },
          );
        }
        const record = {
          id: "124",
          display_name: username,
          username: email,
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
      const emailInput = await screen.findByRole("textbox", { name: "Email" });
      const usernameInput = screen.getByRole("textbox", { name: "Username" });
      const submit = screen.getByRole("button", { name: "Create player" });
      expect(screen.queryByLabelText(/password/i, { selector: "input" })).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /generate password/i })).not.toBeInTheDocument();
      expect(submit).toBeEnabled();

      await user.type(emailInput, "recruit@example");
      await user.type(usernameInput, "TakenName");
      expect(await screen.findByText("That username is already taken.")).toBeVisible();
      expect(submit).toBeDisabled();
      await user.clear(usernameInput);
      await user.type(usernameInput, "no spaces");
      expect(usernameInput).toBeInvalid();
      expect(usernameInput).toHaveAccessibleDescription(
        "Use only English letters (A–Z), digits, and underscores.",
      );
      await user.clear(usernameInput);
      await user.type(usernameInput, username);
      expect(await screen.findByText("Username available.")).toBeVisible();
      await user.click(submit);
      expect(emailInput).toBeInvalid();
      expect(requests).toHaveLength(0);

      await user.clear(emailInput);
      await user.type(emailInput, duplicateEmail);
      await user.click(submit);
      expect(await screen.findByText(conflict)).toBeVisible();
      expect(emailInput).toHaveAccessibleDescription(conflict);
      expect(screen.queryByRole("button", { name: "Copy password" })).not.toBeInTheDocument();

      await user.clear(emailInput);
      await user.type(emailInput, email);
      await user.click(submit);
      expect(await screen.findByText("Player account created.")).toBeVisible();
      const [rejected, created] = requests;
      expect(requests).toHaveLength(2);
      const password = created.body.password;
      expect(created.body).toEqual({ username: email, display_name: username, password });
      expect(password).toHaveLength(24);
      for (const requirement of [/[A-Z]/, /[a-z]/, /[0-9]/, /[!@]/])
        expect(password).toMatch(requirement);
      expect(password).not.toBe(rejected.body.password);
      const signInEmail = screen.getByRole("textbox", { name: "Sign-in email" });
      const generatedPassword = screen.getByRole("textbox", { name: "Generated password" });
      expect(signInEmail).toHaveValue(email);
      expect(generatedPassword).toHaveValue(password);
      expect(signInEmail).toHaveAttribute("readonly");
      expect(generatedPassword).toHaveAttribute("readonly");
      await user.click(screen.getByRole("button", { name: "Copy email" }));
      expect(await screen.findByText("Email copied.")).toBeVisible();
      expect(await navigator.clipboard.readText()).toBe(email);
      await user.click(screen.getByRole("button", { name: "Copy password" }));
      expect(await screen.findByText("Password copied.")).toBeVisible();
      expect(await navigator.clipboard.readText()).toBe(password);
      await user.click(screen.getByRole("button", { name: "Done" }));
      expect(await screen.findByText(username)).toBeVisible();
      for (const [submission, status] of [
        [rejected, 409],
        [created, 201],
      ] as const) {
        const request = telemetry.expectRequest(
          submission.trace,
          "web.admin.create_player",
          "/api/admin/players",
          status,
        );
        expect(request.parent?.attributes["input.email.bytes"]).toBe(
          submission.body.username.length,
        );
        expect(request.parent?.attributes["input.username.bytes"]).toBe(username.length);
      }
      for (const secret of [email, duplicateEmail, password, rejected.body.password]) {
        expect(telemetry.serializedSpanData()).not.toContain(secret);
      }
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
      http.get(`${origin}/crew-books`, () => HttpResponse.json([])),
      http.get(`${origin}/experimental-equipment`, () => HttpResponse.json([])),
      http.get(`${origin}/bounty-equipment`, () => HttpResponse.json([])),
      http.get(`${origin}/improved-equipment`, () => HttpResponse.json([])),
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

  it("grants 2D and 3D styles, crew skins and special recruits with traced requests", async () => {
    const telemetry = captureTraces();
    const grants: Array<{ body: Grant; trace: string | null }> = [];
    const snapshot = structuredClone(garage);
    snapshot.special_recruits[190] = 1;
    const paths = [
      ["styles-2d", { id: 36, name: "Black Widow (France)", max_owned: 1 }],
      ["styles-3d", { id: 506, name: "Proserpina (Carro da Combattimento 45 t)", max_owned: 1 }],
      ["crew-skins", { id: 197, name: "Birthday Skill4ltu", max_owned: null }],
      [
        "special-recruits",
        { id: 74, name: "Miho Nishizumi", countries: [{ id: 74, name: "Germany" }] },
      ],
    ] as const;
    httpServer.use(
      players(),
      http.get(`${origin}/players/123/garage`, () => HttpResponse.json(snapshot)),
      ...[
        "vehicles",
        "attachments",
        "stat-trackers",
        "crew-books",
        "experimental-equipment",
        "bounty-equipment",
        "improved-equipment",
      ].map((path) => http.get(`${origin}/${path}`, () => HttpResponse.json([]))),
      ...paths.map(([path, entry]) =>
        http.get(`${origin}/${path}`, () =>
          HttpResponse.json(
            path === "special-recruits"
              ? [
                  ...Array.from({ length: 101 }, (_, index) => ({
                    id: 200 + index,
                    name: `Recruit ${index}`,
                    countries: [{ id: 200 + index, name: "France" }],
                  })),
                  entry,
                  {
                    id: 190,
                    name: "Birthday14 cat",
                    countries: [
                      { id: 190, name: "France" },
                      { id: 191, name: "Germany" },
                    ],
                  },
                ]
              : path === "styles-2d"
                ? [{ id: 37, name: "Black Widow (Germany)", max_owned: 1 }, entry]
                : [entry],
          ),
        ),
      ),
      http.post(`${origin}/players/123/grants`, async ({ request }) => {
        const body = (await request.json()) as Grant;
        grants.push({ body, trace: request.headers.get("traceparent") });
        if (body.kind === "special_recruit" && grants.length === 4)
          return HttpResponse.json({ message: "The player's barracks are full." }, { status: 409 });
        if (body.kind === "style2d") snapshot.styles_2d[body.id] = body.count;
        if (body.kind === "style3d") snapshot.styles_3d[body.id] = body.count;
        if (body.kind === "crew_skin") snapshot.crew_skins[body.id] = body.count;
        if (body.kind === "special_recruit") snapshot.special_recruits[body.id] = 1;
        return HttpResponse.json({ garage: snapshot, delivery: "on_next_sync" });
      }),
    );
    try {
      const { user } = openAdmin(true);
      await user.click(await screen.findByRole("tab", { name: "Admin" }));
      await user.click(await screen.findByText(player.display_name));
      for (const [label, name] of [
        ["2D style", /Black Widow \(France\)/],
        ["3D style", /Proserpina \(Carro da Combattimento 45 t\)/],
        ["Crew skin", /Birthday Skill4ltu/],
        ["Special recruit", /Miho Nishizumi/],
      ] as const) {
        const picker = await screen.findByRole("combobox", { name: label });
        await user.click(picker);
        if (label === "Special recruit") await user.type(picker, "Miho");
        if (label === "2D style") await user.type(picker, "France");
        if (label === "3D style") await user.type(picker, "Carro");

        await user.click(await screen.findByRole("option", { name }));
        await user.click(screen.getByRole("button", { name: new RegExp(`^Grant ${label}$`, "i") }));
        if (label !== "Special recruit")
          expect(await screen.findByText(/Grant applied/)).toBeVisible();
      }
      expect(await screen.findByText("The player's barracks are full.")).toBeVisible();
      expect(screen.getByRole("combobox", { name: "Country" })).toHaveTextContent("Germany");
      await user.click(screen.getByRole("button", { name: "Grant special recruit" }));
      expect(await screen.findByText(/Grant applied/)).toBeVisible();
      await user.type(screen.getByRole("combobox", { name: "Special recruit" }), "Miho");
      expect(await screen.findByRole("option", { name: /Miho Nishizumi/ })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      const recruitPicker = screen.getByRole("combobox", { name: "Special recruit" });
      await user.clear(recruitPicker);
      await user.type(recruitPicker, "Birthday14");
      await user.click(await screen.findByRole("option", { name: "Birthday14 cat" }));
      const grantRecruit = screen.getByRole("button", { name: "Grant special recruit" });
      expect(grantRecruit).toBeDisabled();
      await user.click(screen.getByRole("combobox", { name: "Country" }));
      expect(screen.getByRole("option", { name: "France · Owned" })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      await user.click(screen.getByRole("option", { name: "Germany" }));
      await user.click(grantRecruit);
      expect(await screen.findByText(/Grant applied/)).toBeVisible();
      expect(grants.map(({ body }) => body)).toEqual([
        { kind: "style2d", id: 36, count: 1 },
        { kind: "style3d", id: 506, count: 1 },
        { kind: "crew_skin", id: 197, count: 1 },
        { kind: "special_recruit", id: 74 },
        { kind: "special_recruit", id: 74 },
        { kind: "special_recruit", id: 191 },
      ]);
      for (const [index, { body, trace }] of grants.entries()) {
        const grant = telemetry.expectRequest(
          trace,
          "web.admin.grant",
          "/api/admin/players/{id}/grants",
          index === 3 ? 409 : 200,
        );
        expect(grant.parent?.attributes["input.grant.kind"]).toBe(body.kind);
      }
    } finally {
      await telemetry.stop();
    }
  });

  it("grants Components, crew books, and equipment from the player garage", async () => {
    const telemetry = captureTraces();
    let snapshot = structuredClone(garage);
    const grants: Array<{ body: Grant; trace: string | null }> = [];
    const book = { id: 286, name: "Training Booklet (U.S.S.R.)" };
    const equipment = { id: 39161, name: "Fire-Control System T3" };
    const improved = { id: 28921, name: "Improved Compressor" };
    const basic = { id: 13049, name: "Bounty Gun Laying Drive · Basic" };
    const upgraded = { id: 13305, name: "Bounty Gun Laying Drive · Upgraded" };
    httpServer.use(
      players(),
      http.get(`${origin}/players/123/garage`, () => HttpResponse.json(snapshot)),
      http.get(`${origin}/vehicles`, () => HttpResponse.json([])),
      http.get(`${origin}/attachments`, () => HttpResponse.json([])),
      http.get(`${origin}/stat-trackers`, () => HttpResponse.json([])),
      http.get(`${origin}/crew-books`, () => HttpResponse.json([book])),
      http.get(`${origin}/experimental-equipment`, () => HttpResponse.json([equipment])),
      http.get(`${origin}/bounty-equipment`, () => HttpResponse.json([basic, upgraded])),
      http.get(`${origin}/improved-equipment`, () => HttpResponse.json([improved])),
      http.post(`${origin}/players/123/grants`, async ({ request }) => {
        const body = (await request.json()) as Grant;
        grants.push({ body, trace: request.headers.get("traceparent") });
        if (grants.length === 1) {
          return HttpResponse.json(
            { message: "The grant would exceed the supported balance limit." },
            { status: 422 },
          );
        }
        if (body.kind === "currency") snapshot = { ...snapshot, equipment_components: "165" };
        if (
          body.kind === "crew_book" ||
          body.kind === "experimental_equipment" ||
          body.kind === "bounty_equipment" ||
          body.kind === "improved_equipment"
        ) {
          snapshot = {
            ...snapshot,
            depot: { ...snapshot.depot, [body.id]: (snapshot.depot[body.id] ?? 0) + body.count },
          };
        }
        return HttpResponse.json({ garage: snapshot, delivery: "queued_for_garage" });
      }),
    );

    try {
      const { user } = openAdmin(true);
      await user.click(await screen.findByRole("tab", { name: "Admin" }));
      await user.click(await screen.findByText(player.display_name));
      const components = await screen.findByText("Experimental Equipment components", {
        selector: "dt",
      });
      expect(components.nextElementSibling).toHaveTextContent("40");
      expect(
        screen.queryByRole("heading", { name: "Grant Experimental Equipment Components" }),
      ).not.toBeInTheDocument();
      await user.click(screen.getByRole("combobox", { name: "Resource" }));
      await user.click(
        await screen.findByRole("option", { name: "Experimental Equipment components" }),
      );
      const amount = screen.getByRole("textbox", { name: "Amount to add" });
      await user.type(amount, "2147483647");
      await user.click(screen.getByRole("button", { name: "Grant resource" }));
      expect(
        await screen.findByText("The grant would exceed the supported balance limit."),
      ).toBeVisible();
      await user.clear(amount);
      await user.type(amount, "125");
      await user.click(screen.getByRole("button", { name: "Grant resource" }));
      expect(await screen.findByText("165", { selector: "dd" })).toBe(
        components.nextElementSibling,
      );

      for (const { picker, button, name, initial, count } of [
        { picker: "Crew book", button: "Grant crew book", name: book.name, initial: 2, count: 3 },
        {
          picker: "Experimental Equipment",
          button: "Grant equipment",
          name: equipment.name,
          initial: 0,
          count: 2,
        },
      ]) {
        const select = screen.getByRole("combobox", { name: picker });
        await user.type(select, name.slice(0, 8));
        await user.click(
          await screen.findByRole("option", { name: (label) => label.startsWith(name) }),
        );
        expect(select).toHaveValue(name);
        const copies = screen.getByRole("textbox", {
          name: "Copies to add",
          description: new RegExp(`^${initial} in depot`),
        });
        await user.clear(copies);
        await user.type(copies, "101");
        const submit = screen.getByRole("button", { name: button });
        expect(submit).toBeDisabled();
        await user.clear(copies);
        await user.type(copies, String(count));
        await user.click(submit);
        expect(
          await screen.findByText(`${initial + count} in depot · Up to 100 per grant`),
        ).toBeVisible();
      }

      const bounty = within(screen.getByRole("form", { name: "Grant Bounty Equipment" }));
      const bountyPicker = bounty.getByRole("combobox", { name: "Bounty Equipment" });
      await user.click(bountyPicker);
      expect(await screen.findByRole("option", { name: /Drive · Basic/ })).toBeVisible();
      await user.click(screen.getByRole("option", { name: /Drive · Upgraded/ }));
      expect(bountyPicker).toHaveValue(upgraded.name);
      await user.click(bounty.getByRole("button", { name: "Grant Bounty Equipment" }));
      expect(await bounty.findByText("1 in depot · Up to 100 per grant")).toBeVisible();

      const improvedForm = within(screen.getByRole("form", { name: "Grant Improved Equipment" }));
      const improvedPicker = improvedForm.getByRole("combobox", { name: "Improved Equipment" });
      await user.type(improvedPicker, "Compressor");
      await user.click(await screen.findByRole("option", { name: /^Improved Compressor/ }));
      await user.click(improvedForm.getByRole("button", { name: "Grant Improved Equipment" }));
      expect(await improvedForm.findByText("1 in depot · Up to 100 per grant")).toBeVisible();

      expect(grants.map(({ body }) => body)).toEqual([
        { kind: "currency", currency: "equipment_components", amount: 2147483647 },
        { kind: "currency", currency: "equipment_components", amount: 125 },
        { kind: "crew_book", id: book.id, count: 3 },
        { kind: "experimental_equipment", id: equipment.id, count: 2 },
        { kind: "bounty_equipment", id: upgraded.id, count: 1 },
        { kind: "improved_equipment", id: improved.id, count: 1 },
      ]);
      for (const [grant, status, expected] of [
        [
          grants[0],
          422,
          { "input.grant.currency": "equipment_components", "input.amount": 2147483647 },
        ],
        [
          grants[1],
          200,
          {
            "input.grant.currency": "equipment_components",
            "output.delivery": "queued_for_garage",
          },
        ],
        [grants[2], 200, { "input.item": book.id, "input.count": 3 }],
        [grants[3], 200, { "input.item": equipment.id, "input.count": 2 }],
        [grants[4], 200, { "input.item": upgraded.id, "input.count": 1 }],
        [grants[5], 200, { "input.item": improved.id, "input.count": 1 }],
      ] as const) {
        const trace = telemetry.expectRequest(
          grant.trace,
          "web.admin.grant",
          "/api/admin/players/{id}/grants",
          status,
        );
        expect(trace.parent?.attributes).toMatchObject({
          "input.grant.kind": grant.body.kind,
          ...expected,
        });
      }
    } finally {
      await telemetry.stop();
    }
  });
});

it("launches a Training battle with selected players and a customized stationary dummy", async () => {
  const telemetry = captureTraces();
  const posts: { trace: string | null; body: unknown }[] = [];
  let optionsTrace: string | null = null;
  let loadoutTrace: string | null = null;
  let wingmateBusy = false;
  httpServer.use(
    players(),
    http.get(`${origin}/battles`, ({ request }) => {
      optionsTrace = request.headers.get("traceparent");
      return HttpResponse.json({
        maps: [{ id: 5, name: "Prokhorovka · Standard Battle" }],
        players: [
          {
            id: 123,
            name: "Commander",
            vehicles: [
              { id: 208, name: "FT", unavailable_reason: "Crew incomplete" },
              { id: 209, name: "T1 Cunningham" },
            ],
          },
          {
            id: 124,
            name: "Wingmate",
            unavailable_reason: wingmateBusy ? "In battle" : null,
            vehicles: [{ id: 210, name: "T-34" }],
          },
          { id: 125, name: "Scout", unavailable_reason: "In battle", vehicles: [] },
        ],
        observer_available: true,
        dummy_vehicles: [{ id: 81, key: "ussr:R04_T-34", name: "T-34", tier: 5 }],
      });
    }),
    http.get(`${origin}/battles/vehicles/81`, ({ request }) => {
      loadoutTrace = request.headers.get("traceparent");
      return HttpResponse.json({
        equipment_slots: 3,
        consumable_slots: 3,
        equipment: [{ id: 120, name: "Improved Hardening" }],
        consumables: [{ id: 121, name: "Small Repair Kit" }],
        crew: [{ name: "Commander", perks: [{ id: 9, name: "Brothers in Arms" }] }],
      });
    }),
    http.post(`${origin}/battles`, async ({ request }) => {
      expect(request.headers.get("x-wot-admin")).toBe("1");
      posts.push({ trace: request.headers.get("traceparent"), body: await request.json() });
      if (posts.length === 1) {
        wingmateBusy = true;
        return HttpResponse.json(
          { message: "A selected player is no longer available." },
          { status: 409 },
        );
      }
      return HttpResponse.json({ battle_id: 7, players_handed_off: 2 });
    }),
  );
  try {
    const { user } = openAdmin(true);
    await user.click(await screen.findByRole("tab", { name: "Admin" }));
    await user.click(await screen.findByRole("link", { name: "Battles" }));
    await user.selectOptions(
      await screen.findByLabelText("Map and battle type"),
      "Prokhorovka · Standard Battle",
    );
    await user.selectOptions(screen.getByLabelText("Battle duration"), "10 minutes");
    const playerPicker = screen.getByRole("combobox", { name: "Add connected player" });
    await user.type(playerPicker, "Scout");
    expect(await screen.findByRole("option", { name: "Scout · In battle" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    for (const name of ["Commander", "Wingmate"]) {
      const picker = screen.getByRole("combobox", { name: "Add connected player" });
      await user.clear(picker);
      await user.type(picker, name);
      await user.click(await screen.findByRole("option", { name }));
    }
    const commanderVehicle = screen.getByLabelText("Vehicle for Commander");
    const unreadyTank = within(commanderVehicle).getByRole("option", {
      name: "FT · Crew incomplete",
    });
    expect(unreadyTank).toBeDisabled();
    expect(commanderVehicle).toHaveValue("209");
    await user.selectOptions(commanderVehicle, unreadyTank);
    expect(commanderVehicle).toHaveValue("209");
    expect(screen.getByRole("button", { name: "Start battle" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Add dummy" }));
    await user.click(screen.getByRole("combobox", { name: "Vehicle for Dummy 1" }));
    await user.click(
      within(await screen.findByRole("listbox")).getByRole("option", { name: "T-34" }),
    );
    await user.click(screen.getByRole("button", { name: "Customize Dummy 1" }));
    const dialog = within(await screen.findByRole("dialog", { name: "Customize Dummy 1" }));
    await user.selectOptions(
      await dialog.findByLabelText("Equipment slot 1"),
      "Improved Hardening",
    );
    await user.selectOptions(dialog.getByLabelText("Consumable slot 1"), "Small Repair Kit");
    await user.click(dialog.getByRole("combobox", { name: "Commander perks (seat 1)" }));
    await user.click(await screen.findByRole("option", { name: "Brothers in Arms" }));
    await user.click(dialog.getByRole("button", { name: "Apply loadout" }));
    await user.click(screen.getByRole("button", { name: "Start battle" }));
    expect(await screen.findByText("A selected player is no longer available.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Refresh players" }));
    expect(await screen.findByText("In battle", { exact: true })).toBeVisible();
    expect(screen.getByLabelText("Vehicle for Wingmate")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Start battle" })).toBeDisabled();
    wingmateBusy = false;
    await user.click(screen.getByRole("button", { name: "Refresh players" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Start battle" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Start battle" }));
    expect(await screen.findByText("Battle started.")).toBeVisible();
    expect(posts).toHaveLength(2);
    expect(posts[1]?.body).toEqual({
      arena_type: 5,
      duration_seconds: 600,
      players: [
        { account_id: 123, team: 1, inventory_id: 209 },
        { account_id: 124, team: 1, inventory_id: 210 },
      ],
      dummies: [
        {
          team: 2,
          loadout: { vehicle: 81, equipment: [120], consumables: [121], crew_perks: [[9]] },
        },
      ],
    });
    telemetry.expectRequest(optionsTrace, "web.admin.battle_options", "/api/admin/battles", 200);
    telemetry.expectRequest(
      loadoutTrace,
      "web.admin.dummy_loadout",
      "/api/admin/battles/vehicles/{vehicle}",
      200,
    );
    telemetry.expectRequest(
      posts[0]?.trace ?? null,
      "web.admin.start_battle",
      "/api/admin/battles",
      409,
    );
    const { parent } = telemetry.expectRequest(
      posts[1]?.trace ?? null,
      "web.admin.start_battle",
      "/api/admin/battles",
      200,
    );
    expect(parent?.attributes).toMatchObject({
      "input.arena_type_id": 5,
      "input.round_length.seconds": 600,
      "input.players.count": 2,
      "input.dummies.count": 1,
      "output.battle.id": 7,
    });
  } finally {
    await telemetry.stop();
  }
});
