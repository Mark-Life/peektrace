/** A drawn transcript block whose event page has not arrived yet: an empty
 *  box of the block's estimated height, so the list does not jump when the
 *  page lands. Jumps and search steps wait while `data-pending` is set. */
export const PendingRow = ({ height }: { readonly height: number }) => (
  <div
    aria-busy="true"
    className="bg-muted/30 rounded-md"
    data-pending=""
    data-testid="history-pending"
    style={{ height: Math.max(height, 0) }}
  />
);
