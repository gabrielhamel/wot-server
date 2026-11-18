import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vite-plus/test";
import { httpServer } from "../../../../tools/web/test/setup";
import { captureTraces } from "../../../../tools/web/test/traces";
import type { Garage, Grant } from "../../application/admin-contracts";
import Admin from "./admin-page";

const origin = "http://localhost:8080/api/admin";
const token = "private-admin-token";
const player = { id: "123", username: "private-login@example.test", display_name: "Commander" };
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
  window.history.replaceState(null, "", "/admin/");
  if (signedIn) sessionStorage.setItem("wot-server.admin-token", token);
  return { user: userEvent.setup(), ...render(<Admin />) };
}

function players(records = [player]) {
  return http.get(`${origin}/players`, ({ request }) =>
    request.headers.get("authorization") === `Bearer ${token}`
      ? HttpResponse.json({ data: records, total: records.length })
      : HttpResponse.json({ message: "A valid admin token is required." }, { status: 401 }),
  );
}

describe("Server Administration", () => {
  it("rejects a bad token, then signs in and lists a player", async () => {
    const telemetry = captureTraces();
    const attempts: Array<{ auth: string | null; trace: string | null }> = [];
    httpServer.use(
      players(),
      http.get(`${origin}/session`, ({ request }) => {
        const auth = request.headers.get("authorization");
        attempts.push({ auth, trace: request.headers.get("traceparent") });
        return auth === `Bearer ${token}`
          ? new HttpResponse(null, { status: 204 })
          : HttpResponse.json({ message: "A valid admin token is required." }, { status: 401 });
      }),
    );
    try {
      const { user } = openAdmin();
      const input = await screen.findByLabelText("Admin token", { exact: false });
      await user.type(input, "bad-token");
      await user.click(screen.getByRole("button", { name: "Sign in" }));
      expect(await screen.findByText("A valid admin token is required.")).toBeVisible();
      await user.clear(input);
      await user.type(input, token);
      await user.click(screen.getByRole("button", { name: "Sign in" }));
      expect(await screen.findByText(player.display_name)).toBeVisible();
      expect(attempts.map((item) => item.auth)).toEqual(["Bearer bad-token", `Bearer ${token}`]);
      const denied = telemetry.expectRequest(
        attempts[0].trace,
        "web.admin.sign_in",
        "/api/admin/session",
        401,
      );
      expect(denied.http?.attributes["output.outcome"]).toBe("rejected");
      telemetry.expectRequest(attempts[1].trace, "web.admin.sign_in", "/api/admin/session", 204);
      expect(telemetry.serializedSpanData()).not.toContain(token);
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
        };
        records.push(record);
        return HttpResponse.json(record, { status: 201 });
      }),
    );
    try {
      const { user } = openAdmin(true);
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
