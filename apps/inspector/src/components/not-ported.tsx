/** Shown in place of a view whose API this server build does not serve yet
 * (the server answered `NotPortedYetError`). No Retry: asking again cannot
 * help until the server is upgraded. */
export const NotPorted = ({ message }: { readonly message: string }) => (
  <p className="text-muted-foreground text-sm" data-testid="not-ported">
    {message}
  </p>
);
