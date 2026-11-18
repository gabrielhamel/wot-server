import { Button } from "@mui/material";
import { useState } from "react";
import {
  Confirm,
  useDataProvider,
  useGetIdentity,
  useNotify,
  useRecordContext,
  useRefresh,
} from "react-admin";
import type { PlayerRecord } from "../../application/admin-contracts";
import type { AdminDataProvider } from "../../infrastructure/admin-api";
import { errorMessage } from "../../../platform/infrastructure/web/http";

export function PlayerRoleAction() {
  const player = useRecordContext<PlayerRecord>();
  const { identity } = useGetIdentity();
  const api = useDataProvider<AdminDataProvider>();
  const notify = useNotify();
  const refresh = useRefresh();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  if (!player || !identity || player.id === identity.id) return null;

  const role = player.role === "admin" ? "player" : "admin";
  const action = role === "admin" ? "Promote to administrator" : "Demote to player";

  async function confirm() {
    if (!player) return;
    setBusy(true);
    try {
      await api.changePlayerRole(player.id, role);
      setOpen(false);
      notify(`${player.display_name}'s role was updated.`, { type: "success" });
      refresh();
    } catch (error) {
      setOpen(false);
      notify(errorMessage(error), { type: "error" });
      refresh();
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Button
        size="small"
        aria-label={`${action} · ${player.display_name}`}
        onClick={(event) => {
          event.stopPropagation();
          setOpen(true);
        }}
      >
        {action}
      </Button>
      <Confirm
        isOpen={open}
        loading={busy}
        title={`Change ${player.display_name}'s role?`}
        content={
          role === "admin"
            ? "This player will gain access to the dashboard."
            : "This player will lose access to the dashboard and be signed out."
        }
        confirm={action}
        onConfirm={confirm}
        onClose={() => setOpen(false)}
      />
    </>
  );
}
