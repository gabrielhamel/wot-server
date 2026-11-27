use super::*;

impl StepState {
    pub(super) fn contacts(
        &mut self,
        world: &mut PhysicsWorld,
        geometry: &mut SceneryColliders,
        tank: &Chassis,
        scenery: &mut Scenery,
        pressure_per_impulse: f32,
        can_damage: bool,
    ) {
        let mut broken_contacts = Vec::new();
        let received_push = world
            .narrow_phase
            .contact_pairs_with(tank.collider)
            .filter(|pair| pair.has_any_active_contact())
            .filter_map(|pair| {
                let first = pair.collider1 == tank.collider;
                let other = if first {
                    pair.collider2
                } else {
                    pair.collider1
                };
                world.colliders.get(other)?.parent()?;
                Some(if first {
                    -pair.total_impulse()
                } else {
                    pair.total_impulse()
                })
            })
            .sum::<Vector>();
        for pair in world
            .narrow_phase
            .contact_pairs_with(tank.collider)
            .filter(|pair| pair.has_any_active_contact())
        {
            let first = pair.collider1 == tank.collider;
            let other = if first {
                pair.collider2
            } else {
                pair.collider1
            };
            let Some(collider) = world.colliders.get(other) else {
                continue;
            };
            if let Some(id) = collider
                .user_data
                .checked_sub(1)
                .and_then(|id| u64::try_from(id).ok())
                .filter(|_| collider.parent().is_some())
            {
                let id = ParticipantId(id);
                if !self.motion.contact_peers.contains(&id) {
                    self.motion.contact_peers.push(id);
                }
                continue;
            }
            if !geometry.contains(other) {
                continue;
            }
            self.motion.scenery_contact = true;
            if !can_damage {
                continue;
            }
            let Some(fragment) = geometry.fragment(other) else {
                continue;
            };
            let Some((manifold, contact)) = pair.find_deepest_contact() else {
                continue;
            };
            let incoming_normal = if first {
                -manifold.data.normal
            } else {
                manifold.data.normal
            };
            let local = if first {
                contact.local_p1
            } else {
                contact.local_p2
            };
            let Some(hull) = world.colliders.get(tank.collider) else {
                continue;
            };
            let point = hull.position().transform_point(local);
            let velocity = self.velocity + self.angular_velocity.cross(point - self.mass_center);
            let speed = (-velocity.dot(incoming_normal)).clamp(0.0, 25.0);
            let normal = game_vec(incoming_normal).xz().normalize_or_zero();
            let press = (-self.press.dot(normal))
                .max(self.pivot_press)
                .max(-game_vec(received_push).xz().dot(normal) * pressure_per_impulse)
                .max(0.0);
            let before = self.damage.speed_scale;
            if self
                .damage
                .damage_contact(scenery, fragment, normal, speed, press)
            {
                let undo_contact = if first {
                    pair.total_impulse()
                } else {
                    -pair.total_impulse()
                };
                broken_contacts.push((
                    other,
                    point,
                    undo_contact,
                    self.damage.speed_scale / before,
                ));
            }
        }
        if !broken_contacts.is_empty() {
            geometry.sync(scenery, &mut world.colliders);
            for (other, point, undo_contact, speed_scale) in broken_contacts {
                if !geometry.contains(other)
                    && let Some(body) = world.bodies.get_mut(tank.body)
                {
                    // Remove the vanished wall's stopping impulse before applying its fracture energy cost.
                    body.apply_impulse_at_point(undo_contact, point, true);
                    let velocity = body.linvel();
                    body.set_linvel(
                        Vector::new(
                            velocity.x * speed_scale,
                            velocity.y,
                            velocity.z * speed_scale,
                        ),
                        true,
                    );
                }
            }
        }
    }
}
