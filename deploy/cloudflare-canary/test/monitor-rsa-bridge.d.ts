declare module "./monitor-rsa-bridge.mjs" {
  export function makeRsaSigner(): any;
  export function createTerminal(fields: any, signer: any): Promise<any>;
}
