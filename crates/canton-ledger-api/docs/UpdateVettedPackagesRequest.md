# UpdateVettedPackagesRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**changes** | Option<[**Vec<models::VettedPackagesChange>**](VettedPackagesChange.md)> | Changes to apply to the current vetting state of the participant on the specified synchronizer. The changes are applied in order. Any package not changed will keep their previous vetting state. | [optional]
**dry_run** | **bool** | If dry_run is true, then the changes are only prepared, but not applied. If a request would trigger an error when run (e.g. TOPOLOGY_DEPENDENCIES_NOT_VETTED), it will also trigger an error when dry_run.  Use this flag to preview a change before applying it. | 
**synchronizer_id** | **String** | If set, the requested changes will take place on the specified synchronizer. If synchronizer_id is unset and the participant is only connected to a single synchronizer, that synchronizer will be used by default. If synchronizer_id is unset and the participant is connected to multiple synchronizers, the request will error out with PACKAGE_SERVICE_CANNOT_AUTODETECT_SYNCHRONIZER.  Optional | 
**expected_topology_serial** | Option<[**models::PriorTopologySerial**](PriorTopologySerial.md)> |  | [optional]
**update_vetted_packages_force_flags** | Option<**Vec<String>**> | Controls whether potentially unsafe vetting updates are allowed.  Optional, defaults to FORCE_FLAG_UNSPECIFIED. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


