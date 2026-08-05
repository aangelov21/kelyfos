import { Modal } from "./Modal";

export interface ConfirmRequest {
  title: string;
  /** Main line: what is about to happen. */
  message: string;
  /** Optional second line for consequences the user cannot see coming. */
  detail?: string;
  /** Label of the confirming button; styled as destructive. */
  confirmLabel?: string;
  onConfirm: () => void;
}

interface Props {
  request: ConfirmRequest;
  onClose: () => void;
}

/**
 * Destructive-action confirmation. Replaces window.confirm, which the webview
 * renders as an unstyled OS sheet and blocks the whole app while it is up.
 */
export function ConfirmDialog({ request, onClose }: Props) {
  const confirm = () => {
    request.onConfirm();
    onClose();
  };

  return (
    <Modal
      title={request.title}
      onClose={onClose}
      width={420}
      footer={
        <>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn danger" autoFocus onClick={confirm}>
            {request.confirmLabel ?? "Delete"}
          </button>
        </>
      }
    >
      <div>{request.message}</div>
      {request.detail && <span className="hint">{request.detail}</span>}
    </Modal>
  );
}
