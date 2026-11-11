//! A location's staff as the back office reads and writes it (ADR-0029, decision 6): its roles,
//! with their permissions and limits, and its team, each member with their role. A PIN is never
//! read back: a member has one or not, and a write sets a new one by giving its digits, which the
//! cell hashes with a salt it draws, or keeps the one they have by giving none. Written staff
//! replace the profile's team and roles, keeping everything else, checked whole by the kernel.

use keel_domain::codec::{Name, ProfileVersion};
use keel_domain::profile::{Member, Profile};
use keel_domain::staff::{Action, Permission, PinHash, Role};
use keel_types::{Entropy, Id};
use serde::{Deserialize, Serialize};

use crate::catalog::hex;

/// A location's staff, as read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffView {
    /// The version of the profile it is part of.
    pub version: String,
    /// Every action a role may permit, and the limits each has.
    pub actions: Vec<ActionJson>,
    /// The roles: empty for a profile without roles, whose members may do everything.
    pub roles: Vec<RoleJson>,
    /// The team.
    pub team: Vec<MemberView>,
}

/// An action, and its limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionJson {
    /// Its name, such as `order.void`.
    pub name: String,
    /// Whether it is limited by an amount.
    pub has_amount: bool,
    /// Whether it is limited by a percentage.
    pub has_percent: bool,
}

/// A role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleJson {
    /// Its identifier.
    pub id: String,
    /// Its name.
    pub name: String,
    /// What its members may do.
    pub permissions: Vec<PermissionJson>,
}

/// An action a role permits, with its limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionJson {
    /// The action's name.
    pub action: String,
    /// The most amount, in minor units of the location's currency; absent for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most_amount: Option<u64>,
    /// The most percentage, in basis points; absent for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most_percent: Option<u32>,
}

/// A team member, as read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    /// Their identifier.
    pub id: String,
    /// Their name.
    pub name: String,
    /// Their role's identifier; absent in a profile without roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Whether they have a PIN.
    pub has_pin: bool,
}

/// Staff to write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StaffEdit {
    /// The version it was read at: if another has been published since, it is refused.
    #[serde(default)]
    pub base: Option<String>,
    /// The roles: none for a profile without roles.
    pub roles: Vec<RoleJson>,
    /// The team.
    pub team: Vec<MemberEdit>,
}

/// A team member to write. Its `Debug` leaves out a new PIN's digits: found in review, a log
/// line of it would have held them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberEdit {
    /// Their identifier.
    pub id: String,
    /// Their name.
    pub name: String,
    /// Their role's identifier: one of the roles written, or absent with none.
    #[serde(default)]
    pub role: Option<String>,
    /// A new PIN, its 4 to 8 digits; absent to keep the one they have.
    #[serde(default)]
    pub pin: Option<String>,
}

impl core::fmt::Debug for MemberEdit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemberEdit")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("role", &self.role)
            .field("pin", &self.pin.as_ref().map(|_| ".."))
            .finish()
    }
}

/// `profile`'s staff, as read.
pub fn view(profile: &Profile, version: ProfileVersion) -> StaffView {
    let data = profile.data();
    StaffView {
        version: hex(version),
        actions: Action::ALL
            .iter()
            .map(|action| ActionJson {
                name: action.name().to_owned(),
                has_amount: action.has_amount(),
                has_percent: action.has_percent(),
            })
            .collect(),
        roles: data
            .roles
            .iter()
            .map(|role| RoleJson {
                id: role.id.to_string(),
                name: role.name.to_string(),
                permissions: role
                    .permissions
                    .iter()
                    .map(|permission| PermissionJson {
                        action: permission.action.name().to_owned(),
                        most_amount: permission.most_amount,
                        most_percent: permission.most_percent,
                    })
                    .collect(),
            })
            .collect(),
        team: data
            .team
            .iter()
            .map(|member| MemberView {
                id: member.id.to_string(),
                name: member.name.to_string(),
                role: member.role.map(|role| role.to_string()),
                has_pin: member.pin.is_some(),
            })
            .collect(),
    }
}

/// Why staff can't be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StaffError {
    /// A field isn't what it must be.
    #[error("{0}")]
    Field(String),
    /// Randomness for a PIN's salt couldn't be had.
    #[error("randomness: {0}")]
    Entropy(String),
    /// The profile it makes breaks a rule.
    #[error("{0}")]
    Profile(String),
}

fn id<T>(text: &str, what: &str) -> Result<Id<T>, StaffError> {
    Id::parse(text).map_err(|_| StaffError::Field(format!("{what} {text:?} isn't an identifier")))
}

fn name(text: &str, what: &str) -> Result<Name, StaffError> {
    Name::new(text).map_err(|_| StaffError::Field(format!("{what} {text:?} isn't a name")))
}

/// A salt of 16 bytes from `entropy`.
fn salt(entropy: &mut impl Entropy) -> Result<[u8; 16], StaffError> {
    let mut salt = [0_u8; 16];
    for half in salt.chunks_mut(8) {
        let word = entropy.next_u64().map_err(|error| StaffError::Entropy(error.to_string()))?;
        half.copy_from_slice(&word.to_be_bytes());
    }
    Ok(salt)
}

/// `profile` with the staff of `edit`, checked whole: new PINs hashed with salts drawn from
/// `entropy`, and members who give none keeping the one they have.
///
/// # Errors
/// [`StaffError`] if a field isn't what it must be, a PIN isn't 4 to 8 digits, or the profile
/// it makes breaks a rule.
pub fn apply(
    profile: &Profile,
    edit: &StaffEdit,
    entropy: &mut impl Entropy,
) -> Result<Profile, StaffError> {
    let mut data = profile.data().clone();
    let roles = edit
        .roles
        .iter()
        .map(|role| {
            Ok(Role {
                id: id(&role.id, "role")?,
                name: name(&role.name, "role name")?,
                permissions: role
                    .permissions
                    .iter()
                    .map(|permission| {
                        Ok(Permission {
                            action: Action::named(&permission.action).ok_or_else(|| {
                                StaffError::Field(format!(
                                    "{:?} isn't an action",
                                    permission.action
                                ))
                            })?,
                            most_amount: permission.most_amount,
                            most_percent: permission.most_percent,
                        })
                    })
                    .collect::<Result<_, StaffError>>()?,
            })
        })
        .collect::<Result<Vec<_>, StaffError>>()?;
    let mut team = Vec::new();
    for member in &edit.team {
        let member_id = id(&member.id, "team member")?;
        let pin =
            match &member.pin {
                Some(digits) => Some(PinHash::new(digits, salt(entropy)?).map_err(|error| {
                    StaffError::Field(format!("{}'s PIN: {error}", member.name))
                })?),
                // Kept, as the member had it: a member new to the team has none.
                None => profile.member(member_id).and_then(|kept| kept.pin),
            };
        let role = member.role.as_deref().map(|role| id(role, "role")).transpose()?;
        // Without roles, no one has a PIN: theirs go with the roles.
        let pin = if roles.is_empty() { None } else { pin };
        team.push(Member {
            id: member_id,
            name: name(&member.name, "team member's name")?,
            role,
            pin,
        });
    }
    data.roles = roles;
    data.team = team;
    Profile::new(data).map_err(|error| StaffError::Profile(error.to_string()))
}

#[cfg(test)]
mod tests {
    use keel_domain::profile::demo;
    use keel_domain::publication::version;
    use keel_types::SeededEntropy;

    use super::*;

    fn edit(view: &StaffView) -> StaffEdit {
        StaffEdit {
            base: Some(view.version.clone()),
            roles: view.roles.clone(),
            team: view
                .team
                .iter()
                .map(|member| MemberEdit {
                    id: member.id.clone(),
                    name: member.name.clone(),
                    role: member.role.clone(),
                    pin: None,
                })
                .collect(),
        }
    }

    #[test]
    fn staff_read_and_written_back_are_the_same_profile() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        assert_eq!(read.roles.len(), 2);
        assert_eq!(read.actions.len(), Action::ALL.len());
        assert!(read.team.iter().all(|member| member.has_pin));
        let written = apply(&profile, &edit(&read), &mut SeededEntropy::new(1)).unwrap();
        assert_eq!(written.encode(), profile.encode(), "PINs kept, nothing changed");
    }

    #[test]
    fn a_new_pin_is_hashed_with_a_salt_drawn_and_a_new_member_needs_one() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        let mut changed = edit(&read);
        changed.team[1].pin = Some("8642".into());
        let written = apply(&profile, &changed, &mut SeededEntropy::new(7)).unwrap();
        let sam = written.data().team[1].id;
        assert!(written.verifies_pin(sam, "8642"));
        assert!(!written.verifies_pin(sam, "2468"));
        // Two draws, two salts: the same PIN hashes apart.
        let again = apply(&profile, &changed, &mut SeededEntropy::new(8)).unwrap();
        assert_ne!(again.data().team[1].pin, written.data().team[1].pin);
        // A member new to the team, with no PIN.
        let mut joined = edit(&read);
        joined.team.push(MemberEdit {
            id: "0192f0c1-0000-7000-8000-000000000503".into(),
            name: "Kim".into(),
            role: Some(read.roles[1].id.clone()),
            pin: None,
        });
        let refused = apply(&profile, &joined, &mut SeededEntropy::new(1));
        assert_eq!(
            refused.map(|_| ()),
            Err(StaffError::Profile("a team member with no PIN".into()))
        );
        joined.team[2].pin = Some("12".into());
        let refused = apply(&profile, &joined, &mut SeededEntropy::new(1));
        assert!(
            matches!(refused, Err(StaffError::Field(message)) if message.contains("Kim's PIN"))
        );
        joined.team[2].pin = Some("1357".into());
        assert!(apply(&profile, &joined, &mut SeededEntropy::new(1)).is_ok());
    }

    #[test]
    fn what_isnt_staff_is_refused_and_says_why() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        let refused = |change: &dyn Fn(&mut StaffEdit)| {
            let mut changed = edit(&read);
            change(&mut changed);
            apply(&profile, &changed, &mut SeededEntropy::new(1)).map(|_| ()).unwrap_err()
        };
        assert!(matches!(
            refused(&|edit| edit.roles[0].permissions[0].action = "order.delete".into()),
            StaffError::Field(message) if message.contains("isn't an action")
        ));
        assert_eq!(
            refused(&|edit| edit.team[0].role = Some("0192f0c1-0000-7000-8000-0000000006ff".into())),
            StaffError::Profile("a reference to a role the profile doesn't have".into())
        );
        assert_eq!(
            refused(&|edit| edit.roles[1].permissions[1].most_percent = Some(10_001)),
            StaffError::Profile("a percentage limit over 100%".into())
        );
        // Every role taken away, with members still in them.
        assert_eq!(
            refused(&|edit| edit.roles.clear()),
            StaffError::Profile("a role or PIN in a profile without roles".into())
        );
    }

    #[test]
    fn a_new_pin_is_never_in_a_log_line() {
        let member = MemberEdit {
            id: "0192f0c1-0000-7000-8000-000000000502".into(),
            name: "Sam".into(),
            role: None,
            pin: Some("8642".into()),
        };
        let shown =
            format!("{:?}", StaffEdit { base: None, roles: Vec::new(), team: vec![member] });
        assert!(!shown.contains("8642"), "{shown}");
        assert!(shown.contains("Sam"));
    }

    #[test]
    fn without_roles_no_one_has_a_pin() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        let mut open = edit(&read);
        open.roles.clear();
        for member in &mut open.team {
            member.role = None;
        }
        let written = apply(&profile, &open, &mut SeededEntropy::new(1)).unwrap();
        assert!(written.data().team.iter().all(|member| member.pin.is_none()));
        let view = view(&written, version(&written));
        assert!(view.roles.is_empty());
        assert!(view.team.iter().all(|member| !member.has_pin && member.role.is_none()));
    }
}
