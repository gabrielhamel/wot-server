export const emailHelp = "Enter a valid email address of up to 128 characters.";
const emailAtom = /^[A-Za-z0-9!#$%&'*+\-/=?^`{|}~]+$/;
const domainLabel = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/;
export const passwordHelp =
  "Use 8 to 128 characters, including at least one digit, one uppercase letter, one lowercase letter, and one symbol.";
const passwordControlHelp = "Control characters are not allowed.";

function validEmail(email: string) {
  const [local, domain, extra] = email.split("@");
  return (
    email.length <= 128 &&
    !email.startsWith("{") &&
    !/\s/.test(email) &&
    extra === undefined &&
    local !== undefined &&
    local.length <= 64 &&
    local.split(".").every((part) => emailAtom.test(part)) &&
    domain !== undefined &&
    domain.includes(".") &&
    domain.split(".").every((label) => domainLabel.test(label))
  );
}

function validPassword(password: string) {
  const length = Array.from(password).length;
  return (
    length >= 8 &&
    length <= 128 &&
    !/\p{Cc}/u.test(password) &&
    /\p{Nd}/u.test(password) &&
    /\p{Lu}/u.test(password) &&
    /\p{Ll}/u.test(password) &&
    /[\p{P}\p{S}]/u.test(password)
  );
}

export function registrationEmailError(value?: string | null) {
  if (!validEmail(value ?? "")) return emailHelp;
}

export function registrationPasswordError(value?: string | null) {
  const password = value ?? "";
  if (!validPassword(password))
    return /\p{Cc}/u.test(password) ? passwordControlHelp : passwordHelp;
}
