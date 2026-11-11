use std::collections::BTreeMap;

use super::sqlite_database;
use crate::{
    db::Database,
    repositories::{
        AuditActor, AuthRepository, CreatePersonalPreset, PersonalPresetRepository,
        PersonalPresetType, RepositoryError, TenantRepository, UserRole,
    },
};

mod rollback;

pub(super) fn input(kind: PersonalPresetType, name: &str) -> CreatePersonalPreset {
    CreatePersonalPreset {
        preset_type: kind,
        name: name.to_owned(),
        version: "2.8.1.55".to_owned(),
        base_id: String::new(),
        inherits: None,
        filament_id: matches!(kind, PersonalPresetType::Filament).then(|| "P123".to_owned()),
        options: BTreeMap::from([("layer_height".to_owned(), "0.2".to_owned())]),
    }
}

#[tokio::test]
async fn personal_presets_are_owner_scoped_and_names_are_global_across_types() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let presets = PersonalPresetRepository::new(database);
    let tenant = tenants.create("preset-owner", "Presets").await.unwrap();
    let owner = auth
        .create_user(tenant.id, "owner@test", "Owner", UserRole::Operator)
        .await
        .unwrap();
    let other = auth
        .create_user(tenant.id, "other@test", "Other", UserRole::Operator)
        .await
        .unwrap();
    let created = presets
        .create_with_audit(
            tenant.id,
            &owner.id,
            input(PersonalPresetType::Print, "Shared"),
            AuditActor::user(&owner.id),
        )
        .await
        .unwrap();

    assert!(
        presets
            .get(tenant.id, &other.id, &created.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !presets
            .delete_with_audit(
                tenant.id,
                &other.id,
                &created.id,
                AuditActor::user(&other.id)
            )
            .await
            .unwrap()
    );
    let error = presets
        .create_with_audit(
            tenant.id,
            &owner.id,
            input(PersonalPresetType::Filament, "Shared"),
            AuditActor::user(&owner.id),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RepositoryError::DuplicatePersonalPresetName
    ));
    let other_created = presets
        .create_with_audit(
            tenant.id,
            &other.id,
            input(PersonalPresetType::Print, "Shared"),
            AuditActor::user(&other.id),
        )
        .await
        .unwrap();
    assert_ne!(created.id, other_created.id);

    let second = presets
        .create_with_audit(
            tenant.id,
            &owner.id,
            input(PersonalPresetType::Print, "Second"),
            AuditActor::user(&owner.id),
        )
        .await
        .unwrap();
    let error = presets
        .replace_with_audit(
            tenant.id,
            &owner.id,
            &second.id,
            input(PersonalPresetType::Printer, "Shared"),
            AuditActor::user(&owner.id),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RepositoryError::DuplicatePersonalPresetName
    ));
}

#[tokio::test]
async fn personal_presets_are_tenant_scoped_even_for_guessed_ids() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let presets = PersonalPresetRepository::new(database);
    let first_tenant = tenants.create("preset-first", "First").await.unwrap();
    let second_tenant = tenants.create("preset-second", "Second").await.unwrap();
    let first = auth
        .create_user(first_tenant.id, "first@test", "First", UserRole::Operator)
        .await
        .unwrap();
    let second = auth
        .create_user(
            second_tenant.id,
            "second@test",
            "Second",
            UserRole::Operator,
        )
        .await
        .unwrap();
    let created = presets
        .create_with_audit(
            first_tenant.id,
            &first.id,
            input(PersonalPresetType::Print, "Private"),
            AuditActor::user(&first.id),
        )
        .await
        .unwrap();
    assert!(
        presets
            .get(second_tenant.id, &second.id, &created.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn durable_owner_clock_survives_deleting_the_final_preset() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let presets = PersonalPresetRepository::new(database);
    let tenant = tenants.create("preset-clock", "Presets").await.unwrap();
    let user = auth
        .create_user(tenant.id, "owner@test", "Owner", UserRole::Operator)
        .await
        .unwrap();
    let first = presets
        .create_with_audit(
            tenant.id,
            &user.id,
            input(PersonalPresetType::Print, "First"),
            AuditActor::user(&user.id),
        )
        .await
        .unwrap();
    presets
        .delete_with_audit(tenant.id, &user.id, &first.id, AuditActor::user(&user.id))
        .await
        .unwrap();
    let second = presets
        .create_with_audit(
            tenant.id,
            &user.id,
            input(PersonalPresetType::Print, "Second"),
            AuditActor::user(&user.id),
        )
        .await
        .unwrap();
    assert!(second.updated_time > first.updated_time);
}

#[tokio::test]
async fn deleting_a_user_cascades_personal_presets_and_owner_clock() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let presets = PersonalPresetRepository::new(database);
    let tenant = tenants.create("preset-cascade", "Presets").await.unwrap();
    let admin = auth
        .create_user(tenant.id, "admin@test", "Admin", UserRole::TenantAdmin)
        .await
        .unwrap();
    let user = auth
        .create_user(tenant.id, "owner@test", "Owner", UserRole::Operator)
        .await
        .unwrap();
    presets
        .create_with_audit(
            tenant.id,
            &user.id,
            input(PersonalPresetType::Print, "Cascade"),
            AuditActor::user(&user.id),
        )
        .await
        .unwrap();
    auth.remove_user_with_audit(tenant.id, &user.id, AuditActor::user(admin.id))
        .await
        .unwrap();
    assert!(
        presets
            .list_metadata(tenant.id, &user.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn personal_preset_map_size_limit_uses_studio_key_value_bytes() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let presets = PersonalPresetRepository::new(database);
    let tenant = tenants.create("preset-size", "Presets").await.unwrap();
    let user = auth
        .create_user(tenant.id, "owner@test", "Owner", UserRole::Operator)
        .await
        .unwrap();
    let mut oversized = input(PersonalPresetType::Print, "Oversized");
    oversized.options = (0..6)
        .map(|index| (format!("option_{index}"), "x".repeat(60 * 1024)))
        .collect();
    assert!(matches!(
        presets
            .create_with_audit(tenant.id, &user.id, oversized, AuditActor::user(&user.id),)
            .await
            .unwrap_err(),
        RepositoryError::PersonalPresetTooLarge
    ));
}
